// Agent 连接检查模块
mod auth;
mod check_agent;

use crate::terminal::auth::check_terminal_connect_permission;
use crate::terminal::check_agent::check_agent;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use nodeget_lib::utils::error_message::generate_error_message;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// 终端消息通道缓冲区大小 - 防止内存耗尽
const TERMINAL_CHANNEL_BUFFER_SIZE: usize = 4096;

// 终端状态结构体，管理 Agent 和 User 之间的会话
// Key 是 (agent_uuid, terminal_id)
#[derive(Clone)]
pub struct TerminalState {
    // 存储终端会话的并发安全映射
    pub sessions: Arc<RwLock<HashMap<TerminalSessionKey, SessionSlots>>>,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct TerminalSessionKey {
    pub agent_uuid: Uuid,
    pub terminal_id: Uuid,
}

// 会话槽位结构，Agent 连接时创建，User 连接时取走需要的部分
//
// Agent 连接时创建这个结构，User 连接时取走需要的部分
pub struct SessionSlots {
    // User -> Agent 的有界发送通道，防止内存耗尽
    pub tx_to_agent: mpsc::Sender<Message>,

    // Agent -> User 的有界接收通道，可选参数
    pub rx_from_agent: Option<mpsc::Receiver<Message>>,

    // 任务令牌
    pub task_token: String,
}

// 终端参数结构体，用于解析 WebSocket 连接参数
#[derive(Deserialize)]
pub struct TerminalParams {
    // Agent 的 UUID
    pub agent_uuid: String,

    pub task_id: Option<u64>,       // 任务ID
    pub task_token: Option<String>, // Task Token
    pub terminal_id: Option<Uuid>,  // 终端连接 ID

    pub token: Option<String>,
}

// 终端 WebSocket 处理器
//
// # 参数
// * `ws` - WebSocket 升级实例
// * `Query(params)` - 查询参数
// * `State(state)` - 终端状态
//
// # 返回值
// 返回可转换为响应的类型
pub async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<TerminalParams>,
    State(state): State<TerminalState>,
) -> impl IntoResponse {
    debug!(target: "terminal", agent_uuid = %params.agent_uuid, "WebSocket upgrade request");
    ws.on_upgrade(move |socket| handle_socket(socket, params, state))
}

// 处理 WebSocket 连接
//
// # 参数
// * `socket` - WebSocket 连接实例
// * `params` - 终端参数
// * `state` - 终端状态
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

// 处理 Agent 连接
//
// # 参数
// * `socket` - WebSocket 连接实例
// * `agent_uuid` - Agent 的 UUID
// * `task_token` - 任务令牌
// * `id` - 任务 ID
// * `state` - 终端状态
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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
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
    {
        use std::collections::hash_map::Entry;
        let mut sessions = state.sessions.write().await;

        // Entry API 确保检查和插入是原子操作
        match sessions.entry(session_key.clone()) {
            Entry::Occupied(_) => {
                // session 已存在，返回错误
                let error_json = generate_error_message(
                    108,
                    &format!(
                        "Invalid Input: terminal_id '{terminal_id}' is already active for this agent"
                    ),
                );
                if let Err(e) = socket
                    .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
                    .await
                {
                    error!(target: "terminal", error = %e, "Failed to send error message to agent");
                }
                return;
            }
            Entry::Vacant(entry) => {
                // session 不存在，安全插入
                entry.insert(SessionSlots {
                    tx_to_agent,                        // User 将会获取这个 Sender 发送数据给 Agent
                    rx_from_agent: Some(rx_from_agent), // User 将会拿走这个 Receiver 接收 Agent 的数据
                    task_token,
                });
            }
        }
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // 从 User 接收数据 -> 发送给 Agent WS
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = rx_from_user.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 从 Agent WS 接收数据 -> 发送给 User
    let send_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if tx_to_user.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 等待 WebSocket 断开
    let _ = send_task.await;
    recv_task.abort();

    // 清理 Map - 直接删除，无需检查（remove操作本身就是幂等的）
    {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_key);
    }
    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "Agent terminal disconnected");
}

// 处理 User 连接
//
// # 参数
// * `socket` - WebSocket 连接实例
// * `agent_uuid` - Agent 的 UUID
// * `token` - 用户令牌
// * `state` - 终端状态
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
    let (tx_to_agent, rx_from_agent) = {
        let Ok(parsed_uuid) = Uuid::parse_str(&agent_uuid) else {
            warn!(target: "terminal", agent_uuid = %agent_uuid, "User connection rejected: invalid UUID format");
            return;
        };
        let session_key = TerminalSessionKey {
            agent_uuid: parsed_uuid,
            terminal_id,
        };
        let mut sessions = state.sessions.write().await;
        if let Some(slots) = sessions.get_mut(&session_key) {
            if let Some(rx) = slots.rx_from_agent.take() {
                (slots.tx_to_agent.clone(), rx)
            } else {
                warn!(
                    target: "terminal",
                    agent_uuid = %agent_uuid,
                    terminal_id = %terminal_id,
                    "Terminal session already has an attached user"
                );
                return;
            }
        } else {
            warn!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "Terminal session not found");
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
        _ = recv_task => {},
        _ = send_task => {},
    }

    info!(target: "terminal", agent_uuid = %agent_uuid, terminal_id = %terminal_id, "User terminal disconnected");
}

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
