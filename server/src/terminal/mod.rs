// Agent 连接检查模块
mod auth;
mod check_agent;

use crate::terminal::auth::check_terminal_connect_permission;
use crate::terminal::check_agent::check_agent;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use nodeget_lib::utils::error_message::generate_error_message;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

// 终端状态结构体，管理 Agent 和 User 之间的会话
// Key 是 (agent_uuid, terminal_id)
#[derive(Clone)]
pub struct TerminalState {
    // 存储终端会话的并发安全映射
    pub sessions: Arc<RwLock<HashMap<TerminalSessionKey, SessionSlots>>>,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct TerminalSessionKey {
    pub agent_uuid: String,
    pub terminal_id: Uuid,
}

// 会话槽位结构，Agent 连接时创建，User 连接时取走需要的部分
//
// Agent 连接时创建这个结构，User 连接时取走需要的部分
pub struct SessionSlots {
    // User -> Agent 的无界发送通道
    pub tx_to_agent: mpsc::UnboundedSender<Message>,

    // Agent -> User 的无界接收通道，可选参数
    pub rx_from_agent: Option<mpsc::UnboundedReceiver<Message>>,

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
    ws.on_upgrade(move |socket| handle_socket(socket, params, state))
}

// 处理 WebSocket 连接
//
// # 参数
// * `socket` - WebSocket 连接实例
// * `params` - 终端参数
// * `state` - 终端状态
async fn handle_socket(socket: WebSocket, params: TerminalParams, state: TerminalState) {
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
                error!("Failed to send error message to agent: {e}");
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
                error!("Failed to send error message to agent: {e}");
            }
            return;
        }
    }

    let session_key = TerminalSessionKey {
        agent_uuid: agent_uuid.clone(),
        terminal_id,
    };

    info!("Agent connecting terminal: agent_uuid={agent_uuid}, terminal_id={terminal_id}");

    // User -> Agent
    let (tx_to_agent, mut rx_from_user) = mpsc::unbounded_channel::<Message>();
    // Agent -> User
    let (tx_to_user, rx_from_agent) = mpsc::unbounded_channel::<Message>();

    // 存入 Map
    {
        let mut sessions = state.sessions.write().await;
        if sessions.contains_key(&session_key) {
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
                error!("Failed to send error message to agent: {e}");
            }
            return;
        }
        sessions.insert(
            session_key.clone(),
            SessionSlots {
                tx_to_agent,                        // User 将会获取这个 Sender 发送数据给 Agent
                rx_from_agent: Some(rx_from_agent), // User 将会拿走这个 Receiver 接收 Agent 的数据
                task_token,
            },
        );
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
            if tx_to_user.send(msg).is_err() {
                break;
            }
        }
    });

    // 等待 WebSocket 断开
    let _ = send_task.await;
    recv_task.abort();

    // 清理 Map
    {
        let mut sessions = state.sessions.write().await;
        if sessions.contains_key(&session_key) {
            sessions.remove(&session_key);
        }
    }
    info!("Agent terminal disconnected: agent_uuid={agent_uuid}, terminal_id={terminal_id}");
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
        warn!("User connection rejected: missing terminal_id");
        let error_json = generate_error_message(
            108,
            "Invalid Input: Missing terminal_id for user terminal connection",
        );
        let _ = socket
            .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
            .await;
        return;
    };

    info!("User connecting terminal to: agent_uuid={agent_uuid}, terminal_id={terminal_id}");

    // 检查 token 是否存在
    let Some(token) = token else {
        warn!("User connection rejected: missing token");
        return;
    };

    // 检查 Terminal Connect 权限
    if let Err(e) = check_terminal_connect_permission(&token, &agent_uuid).await {
        warn!("User connection rejected: {e}");
        return;
    }

    // 获取会话槽位
    let (tx_to_agent, rx_from_agent) = {
        let session_key = TerminalSessionKey {
            agent_uuid: agent_uuid.clone(),
            terminal_id,
        };
        let mut sessions = state.sessions.write().await;
        if let Some(slots) = sessions.get_mut(&session_key) {
            if let Some(rx) = slots.rx_from_agent.take() {
                (slots.tx_to_agent.clone(), rx)
            } else {
                warn!(
                    "Terminal session already has an attached user: agent_uuid={agent_uuid}, terminal_id={terminal_id}"
                );
                return;
            }
        } else {
            warn!("Terminal session not found: agent_uuid={agent_uuid}, terminal_id={terminal_id}");
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
            if tx_to_agent.send(msg).is_err() {
                break;
            }
        }
    });

    tokio::select! {
        _ = recv_task => {},
        _ = send_task => {},
    }

    info!("User terminal disconnected: agent_uuid={agent_uuid}, terminal_id={terminal_id}");
}

async fn reject_with_error(mut socket: WebSocket, error_id: i32, message: &str) {
    let error_json = generate_error_message(error_id, message);
    if let Err(e) = socket
        .send(Message::Text(Utf8Bytes::from(error_json.to_string())))
        .await
    {
        error!("Failed to send terminal error message: {e}");
    }
}
