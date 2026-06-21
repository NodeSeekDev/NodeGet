//! Terminal WebSocket 中继模块。
//!
//! 职责：在 User（浏览器终端）和 Agent（远程 Shell）之间建立双向 WebSocket 中继，
//! 通过 mpsc 通道解耦两端的收发。
//!
//! 连接流程：
//! 1. Agent 发起 WebSocket 连接，携带 `task_token` + `task_id` + `terminal_id`
//! 2. 校验 Agent 身份（通过 `task` 表验证 WebShell 任务）
//! 3. Agent 创建 `SessionSlots` 并注册到 `TerminalState`
//! 4. User 发起 WebSocket 连接，携带 `token` + `terminal_id`
//! 5. 校验 User 权限（Token 需持有 `Terminal::Connect` 权限）
//! 6. User 从 `SessionSlots` 取走 `rx_from_agent`，建立双向通道
//!
//! 协作关系：`auth` 模块校验 User 权限，`check_agent` 模块校验 Agent 身份，
//! 服务器二进制通过 `router()` 挂载到 axum Router。

use crate::auth::check_terminal_connect_permission;
use crate::check_agent::check_agent;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use ng_core::error::anyhow_to_nodeget_error;
use ng_core::utils::error_message::generate_error_message;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// 终端消息通道缓冲区大小，防止无界通道导致内存耗尽。
const TERMINAL_CHANNEL_BUFFER_SIZE: usize = 4096;

/// 终端全局状态，管理所有活跃的 Agent-User 会话。
///
/// Key 为 `(agent_uuid, terminal_id)` 组合，保证每个 Agent 下的每个终端会话唯一。
#[derive(Clone)]
pub struct TerminalState {
    /// 终端会话的并发安全映射，`RwLock` 保护。
    pub sessions: Arc<RwLock<HashMap<TerminalSessionKey, SessionSlots>>>,
}

/// 终端会话标识，由 Agent UUID 和终端 ID 组成。
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct TerminalSessionKey {
    /// Agent 的 UUID 标识。
    pub agent_uuid: Uuid,
    /// 终端会话 ID，由 Agent 在连接时生成。
    pub terminal_id: Uuid,
}

/// 会话槽位：Agent 连接时创建，User 连接时取走需要的通道。
///
/// Agent 持有 `tx_to_agent`（向自身 WebSocket 转发 User 消息）和
/// `rx_from_agent`（接收自身 WebSocket 消息转给 User）。
/// User 连接时 `take()` 走 `rx_from_agent`，与 `tx_to_agent.clone()` 一起使用。
pub struct SessionSlots {
    /// User -> Agent 的有界发送通道，Agent 端持有时用于接收 User 消息。
    pub tx_to_agent: mpsc::Sender<Message>,

    /// Agent -> User 的有界接收通道，User 连接时 take 走。
    /// 若为 `None`，说明该会话已有 User 占用。
    pub rx_from_agent: Option<mpsc::Receiver<Message>>,

    /// 任务令牌，用于验证 Agent 连接合法性。
    pub task_token: String,
}

/// WebSocket 连接参数，从查询字符串解析。
#[derive(Deserialize)]
pub struct TerminalParams {
    /// Agent 的 UUID 字符串。
    pub agent_uuid: String,

    /// 任务 ID，Agent 连接时必传。
    pub task_id: Option<u64>,

    /// 任务令牌，Agent 连接时必传。
    pub task_token: Option<String>,

    /// 终端连接 ID，Agent 和 User 连接时均必传。
    pub terminal_id: Option<Uuid>,

    /// User 的认证 Token，User 连接时必传。
    pub token: Option<String>,
}

/// Build and return an axum Router for the WebSocket terminal endpoint.
///
/// Route:
/// - `/terminal` — WebSocket terminal relay between user and agent
pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/terminal", axum::routing::get(terminal_ws_handler))
        .with_state(TerminalState {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
}

/// Terminal WebSocket 升级处理器。
///
/// - `ws` - WebSocket 升级请求实例
/// - `Query(params)` - 查询字符串解析的连接参数
/// - `State(state)` - 共享的终端会话状态
///
/// 返回：WebSocket 升级响应。
///
/// 内部步骤：
/// 1. 设置最大帧大小 1MB、最大消息大小 4MB（防止 oversized frame/message）
/// 2. 升级为 WebSocket 后委托给 [`handle_socket`]
pub async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<TerminalParams>,
    State(state): State<TerminalState>,
) -> impl IntoResponse {
    debug!(target: "terminal", agent_uuid = %params.agent_uuid, "WebSocket upgrade request");
    ws.max_frame_size(1024 * 1024)
        .max_message_size(4 * 1024 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, params, state))
}

/// 处理已升级的 WebSocket 连接，根据参数分发到 Agent 或 User 处理逻辑。
///
/// - `socket` - 已升级的 WebSocket 连接
/// - `params` - 连接参数
/// - `state` - 共享的终端会话状态
///
/// 分发逻辑：携带 `task_token` + `task_id` 的是 Agent，否则是 User。
async fn handle_socket(socket: WebSocket, params: TerminalParams, state: TerminalState) {
    debug!(target: "terminal", "routing terminal connection");
    let TerminalParams {
        agent_uuid,
        task_id,
        task_token,
        terminal_id,
        token,
    } = params;

    // 有 task_token 的是 Agent，否则是 User
    if let (Some(task_token), Some(id)) = (task_token, task_id) {
        if let Some(terminal_id) = terminal_id {
            handle_agent(socket, agent_uuid, terminal_id, task_token, id, state).await;
        } else {
            reject_with_error(
                socket,
                108,
                "Invalid Input: Missing terminal_id for agent terminal connection",
            )
            .await;
        }
    } else {
        handle_user(socket, agent_uuid, terminal_id, token, state).await;
    }
}

/// 处理 Agent 端的 WebSocket 连接。
///
/// - `socket` - WebSocket 连接实例
/// - `agent_uuid` - Agent 的 UUID 字符串
/// - `terminal_id` - 终端会话 ID
/// - `task_token` - 任务令牌，用于验证连接合法性
/// - `id` - 任务 ID
/// - `state` - 共享的终端会话状态
///
/// 内部步骤：
/// 1. 调用 [`check_agent`] 验证 Agent 身份（task_id + uuid + token 匹配且为 WebShell）
/// 2. 解析 agent_uuid 为 [`Uuid`]
/// 3. 创建双向 mpsc 通道（User->Agent 和 Agent->User）
/// 4. 使用 Entry API 原子插入 `SessionSlots`，避免 TOCTOU 竞态
/// 5. 启动两个异步任务：从 User 通道转发到 Agent WS、从 Agent WS 转发到 User 通道
/// 6. WebSocket 断开后清理会话映射
async fn handle_agent(
    mut socket: WebSocket,
    agent_uuid: String,
    terminal_id: Uuid,
    task_token: String,
    id: u64,
    state: TerminalState,
) {
    match check_agent(agent_uuid.clone(), task_token.clone(), id).await {
        Ok(true) => {}
        Ok(false) => {
            let error_json =
                generate_error_message(102, "Permission Denied: Invalid Task Token or ID");

            if let Err(e) = socket
                .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
                .await
            {
                error!(target: "terminal", error = %e, "Failed to send error message to agent");
            }
            return;
        }
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            let error_json =
                generate_error_message(nodeget_err.error_code(), &format!("{nodeget_err}"));

            if let Err(e) = socket
                .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
                .await
            {
                error!(target: "terminal", error = %e, "Failed to send error message to agent");
            }
            return;
        }
    }

    let Ok(parsed_uuid) = Uuid::parse_str(&agent_uuid) else {
        reject_with_error(socket, 108, "Invalid Agent UUID format").await;
        return;
    };

    let session_key = TerminalSessionKey {
        agent_uuid: parsed_uuid,
        terminal_id,
    };

    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "Agent connecting terminal");

    // User -> Agent - 使用有界通道防止内存耗尽
    let (tx_to_agent, mut rx_from_user) = mpsc::channel::<Message>(TERMINAL_CHANNEL_BUFFER_SIZE);
    // Agent -> User - 使用有界通道防止内存耗尽
    let (tx_to_user, rx_from_agent) = mpsc::channel::<Message>(TERMINAL_CHANNEL_BUFFER_SIZE);

    // 存入 Map - 使用 Entry API 避免 TOCTOU 竞态条件
    // std::sync::RwLockWriteGuard 不是 Send，必须在独立作用域内完成，
    // 不能跨 .await 持有，因此先在同步块内完成插入判定，再在外面处理错误。
    let insert_ok = {
        use std::collections::hash_map::Entry;
        let mut sessions = state
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match sessions.entry(session_key.clone()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(SessionSlots {
                    tx_to_agent,                        // User 将会获取这个 Sender 发送数据给 Agent
                    rx_from_agent: Some(rx_from_agent), // User 将会拿走这个 Receiver 接收 Agent 的数据
                    task_token,
                });
                true
            }
        }
    };
    if !insert_ok {
        let error_json = generate_error_message(
            108,
            &format!("Invalid Input: terminal_id '{terminal_id}' is already active for this agent"),
        );
        if let Err(e) = socket
            .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
            .await
        {
            error!(target: "terminal", error = %e, "Failed to send error message to agent");
        }
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // 从 User 接收数据 -> 发送给 Agent WS
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = rx_from_user.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 从 Agent WS 接收数据 -> 发送给 User
    let mut send_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if tx_to_user.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 等待任一方向结束：User 断开（recv_task 的 rx_from_user 收到 None）
    // 或 Agent WS 断开（send_task 的 ws_reader 结束）。
    // 原实现只 `send_task.await`：User 断开时 recv_task 退出但 send_task 仍阻塞在
    // ws_reader.next()，导致 handle_agent 不返回、session 残留、rx_from_agent 已被
    // take 走，后续 User 重连同一 (agent_uuid, terminal_id) 被 "AlreadyAttached" 拒绝，
    // 且 Agent WS 连接 + send_task 永久悬挂泄漏。改用 select! 任一结束即清理。
    tokio::select! {
        biased;
        _ = &mut recv_task => {},
        _ = &mut send_task => {},
    }
    // 主动 abort 另一个仍可能挂起的任务，确保无悬挂 task
    recv_task.abort();
    send_task.abort();

    // 清理会话映射：remove 本身幂等，无需先 contains_key 再删
    {
        let mut sessions = state
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.remove(&session_key);
    }
    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "Agent terminal disconnected");
}

/// 处理 User 端的 WebSocket 连接。
///
/// - `socket` - WebSocket 连接实例
/// - `agent_uuid` - 目标 Agent 的 UUID 字符串
/// - `terminal_id` - 终端会话 ID（必须提供）
/// - `token` - User 的认证 Token（必须提供）
/// - `state` - 共享的终端会话状态
///
/// 内部步骤：
/// 1. 校验 `terminal_id` 和 `token` 必须存在
/// 2. 调用 [`check_terminal_connect_permission`] 验证 User 权限
/// 3. 从 `TerminalState` 中取出对应的 `SessionSlots`，拿走 `rx_from_agent`
/// 4. 启动两个异步任务：从 Agent 通道转发到 User WS、从 User WS 转发到 Agent 通道
/// 5. 两个任务任一结束时断开连接
async fn handle_user(
    mut socket: WebSocket,
    agent_uuid: String,
    terminal_id: Option<Uuid>,
    token: Option<String>,
    state: TerminalState,
) {
    let Some(terminal_id) = terminal_id else {
        warn!(target: "terminal", "User connection rejected: missing terminal_id");
        let error_json = generate_error_message(
            108,
            "Invalid Input: Missing terminal_id for user terminal connection",
        );
        let _ = socket
            .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
            .await;
        return;
    };

    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "User connecting terminal");

    // 检查 token 是否存在
    let Some(token) = token else {
        warn!(target: "terminal", "User connection rejected: missing token");
        let _ = socket
            .send(Message::Text(Utf8Bytes::from(
                generate_error_message(
                    108,
                    "Invalid Input: Missing token for user terminal connection",
                )
                .to_string(),
            )))
            .await;
        return;
    };

    // 检查 Terminal Connect 权限
    if let Err(e) = check_terminal_connect_permission(&token, &agent_uuid).await {
        warn!(target: "terminal", error = %e, "User connection rejected");
        let error_json = generate_error_message(
            102,
            "Permission Denied: Terminal connection permission denied",
        );
        let _ = socket
            .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
            .await;
        return;
    }

    // 获取会话槽位
    enum SlotResult {
        Got(mpsc::Sender<Message>, mpsc::Receiver<Message>),
        AlreadyAttached,
        NotFound,
    }
    let slot_result = {
        let Ok(parsed_uuid) = Uuid::parse_str(&agent_uuid) else {
            warn!(target: "terminal", agent_uuid = %agent_uuid, "User connection rejected: invalid UUID format");
            return;
        };
        let session_key = TerminalSessionKey {
            agent_uuid: parsed_uuid,
            terminal_id,
        };
        let mut sessions = state
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slots) = sessions.get_mut(&session_key) {
            if let Some(rx) = slots.rx_from_agent.take() {
                SlotResult::Got(slots.tx_to_agent.clone(), rx)
            } else {
                SlotResult::AlreadyAttached
            }
        } else {
            SlotResult::NotFound
        }
    };
    let (tx_to_agent, rx_from_agent) = match slot_result {
        SlotResult::Got(tx, rx) => (tx, rx),
        SlotResult::AlreadyAttached => {
            warn!(
                target: "terminal",
                agent_uuid = %agent_uuid,
                terminal_id = %terminal_id,
                "Terminal session already has an attached user"
            );
            let _ = socket
                .send(Message::Text(Utf8Bytes::from(
                    generate_error_message(108, "Terminal session already has an attached user")
                        .to_string(),
                )))
                .await;
            return;
        }
        SlotResult::NotFound => {
            warn!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "Terminal session not found");
            let _ = socket
                .send(Message::Text(Utf8Bytes::from(
                    generate_error_message(108, "Terminal session not found").to_string(),
                )))
                .await;
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut rx_from_agent = rx_from_agent;

    let recv_task = tokio::spawn(async move {
        while let Some(msg) = rx_from_agent.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if tx_to_agent.send(msg).await.is_err() {
                break;
            }
        }
    });

    tokio::select! {
        biased;
        _ = recv_task => {},
        _ = send_task => {},
    }

    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "User terminal disconnected");
}

/// 向 WebSocket 连接发送错误消息后关闭。
///
/// - `socket` - WebSocket 连接实例
/// - `error_id` - 错误代码，用于构造 JSON 错误消息
/// - `message` - 错误描述文本
async fn reject_with_error(mut socket: WebSocket, error_id: i32, message: &str) {
    warn!(target: "terminal", error_id = error_id, message = %message, "rejecting WebSocket with error");
    let error_json = generate_error_message(error_id, message);
    if let Err(e) = socket
        .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
        .await
    {
        error!(target: "terminal", error = %e, "Failed to send terminal error message");
    }
}
