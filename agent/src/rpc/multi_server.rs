use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::AGENT_CONFIG;
use crate::rpc::wrap_json_into_rpc_with_id_1;
use futures::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use nodeget_lib::config::agent::Server;
use nodeget_lib::error::NodegetError;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{OnceCell, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Agent 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// 服务器连接句柄，包含上行和下行消息通道
pub struct ServerHandle {
    uplink_tx: broadcast::Sender<Message>, // 上行消息发送器（客户端到服务器）
    downlink_tx: broadcast::Sender<Message>, // 下行消息发送器（服务器到客户端）
}

// 全局连接池，存储与各个服务器的连接句柄
static CONNECTION_POOL: OnceCell<RwLock<HashMap<String, Arc<ServerHandle>>>> =
    OnceCell::const_new();

// 初始化与多个服务器的连接
//
// 为每个配置的服务器创建连接管理器任务和相应的消息通道
//
// # 参数
// * `servers` - 服务器配置向量
pub async fn init_connections(servers: Vec<Server>) -> Vec<JoinHandle<()>> {
    let mut map = HashMap::new();
    let mut handles = Vec::new();

    for server in servers {
        let (uplink_tx, uplink_rx) = broadcast::channel::<Message>(32);

        let (downlink_tx, _) = broadcast::channel::<Message>(32);

        let handle = ServerHandle {
            uplink_tx,
            downlink_tx: downlink_tx.clone(),
        };

        map.insert(server.name.clone(), Arc::new(handle));

        handles.push(tokio::spawn(connection_manager(
            server,
            uplink_rx,
            downlink_tx,
        )));
    }

    if let Some(pool) = CONNECTION_POOL.get() {
        let mut guard = pool.write().await;
        *guard = map;
        info!("Connection pool refreshed");
    } else {
        if CONNECTION_POOL.set(RwLock::new(map)).is_err() {
            warn!("Connection pool initialization raced; reusing existing pool");
        }
        info!("Connection pool initialized");
    }

    handles
}

// 连接生命周期维护
//
// 管理与单个服务器的 WebSocket 连接，包括连接建立、任务注册、消息转发和自动重连
//
// # 参数
// * `server` - 服务器配置
// * `uplink_rx` - 上行消息接收器
// * `downlink_tx` - 下行消息发送器
async fn connection_manager(
    server: Server,
    mut uplink_rx: broadcast::Receiver<Message>,
    downlink_tx: broadcast::Sender<Message>,
) {
    // 临时定义用于检测 JsonRpc 长连接错误
    #[derive(Deserialize)]
    struct JsonRpcErrorCheck {
        error: Option<JsonRpcErrorDetail>,
    }

    #[derive(Deserialize)]
    struct JsonRpcErrorDetail {
        code: i64,
        message: String,
    }

    let name = &server.name;
    let token = &server.token;
    let url = &server.ws_url;

    info!("[{name}] Manager task started");

    loop {
        info!("[{name}] Connecting to {url}...");

        let ws_stream = match connect_with_retry(name, url).await {
            Ok(ws) => ws,
            Err(e) => {
                error!("[{name}] Failed to connect: {e}");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        info!("[{name}] Connected successfully");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // 任务注册
        {
            if server.allow_task.unwrap_or(false) {
                let rpc = wrap_json_into_rpc_with_id_1(
                    "task_register_task",
                    vec![
                        serde_json::Value::String(token.clone()),
                        serde_json::Value::String(
                            AGENT_CONFIG
                                .get()
                                .expect("Agent config not initialized")
                                .read()
                                .expect("AGENT_CONFIG lock poisoned")
                                .agent_uuid
                                .to_string(),
                        ),
                    ],
                );

                if let Err(e) = ws_write.send(Message::Text(Utf8Bytes::from(rpc))).await {
                    error!(
                        "[{name}] Write error (register task listener): {e}, triggering reconnect..."
                    );
                    continue;
                }
                debug!("[{name}] Task register request sent.");
            }
        }

        loop {
            tokio::select! {
                // Channel -> WebSocket (上行数据)
                msg_res = uplink_rx.recv() => {
                    match msg_res {
                        Ok(msg) => {
                            // 正常收到消息，发送给 WebSocket
                            if let Err(e) = ws_write.send(msg).await {
                                error!("[{name}] Write error: {e}, triggering reconnect...");
                                break;
                            }
                        }
                        Err(RecvError::Lagged(skipped_count)) => {
                            warn!("[{name}] Connection lagged, dropped {skipped_count} old messages. Creating space for new data.");
                        }
                        Err(RecvError::Closed) => {
                            info!("[{name}] Channel closed, manager task exiting.");
                            return;
                        }
                    }
                }

                // WebSocket -> Broadcast Channel (下行数据)
                ws_msg_opt = ws_read.next() => {
                    match ws_msg_opt {
                        Some(Ok(msg)) => {
                            if let Message::Text(text) = &msg
                                && let Ok(check) = serde_json::from_str::<JsonRpcErrorCheck>(text)
                                    && let Some(err) = check.error {
                                        error!("[{name}] RPC Error Response: {}: {}", err.code, err.message);
                                    }
                            let _ = downlink_tx.send(msg);
                        }
                        Some(Err(e)) => {
                            error!("[{name}] Read error: {e}, triggering reconnect...");
                            break;
                        }
                        None => {
                            warn!("[{name}] Connection closed by server, triggering reconnect...");
                            break;
                        }
                    }
                }
            }
        }

        warn!("[{name}] Disconnected. Waiting 3s before reconnecting...");
        sleep(Duration::from_secs(3)).await;
    }
}

// 带重试机制的 WebSocket 连接
//
// 尝试连接到指定的 WebSocket URL，如果失败则进行重试
//
// # 参数
// * `name` - 服务器名称（用于日志）
// * `url` - WebSocket URL
//
// # 返回值
// 成功时返回 WebSocket 流，失败时返回错误
async fn connect_with_retry(
    name: &str,
    url: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let mut retry_count = 0;
    loop {
        match timeout(Duration::from_secs(5), connect_async(url)).await {
            Ok(Ok((ws_stream, _))) => return Ok(ws_stream),
            Ok(Err(e)) => {
                warn!("[{name}] Connect failed: {e}");
            }
            Err(_) => {
                warn!("[{name}] Connect timeout");
            }
        }

        retry_count += 1;
        if retry_count >= 30 {
            return Err(NodegetError::AgentConnectionError(format!(
                "Failed to connect to {name} after {retry_count} retries"
            )));
        }
        let wait_secs = if retry_count < 5 { 2 } else { 5 };
        debug!("[{name}] Retry attempt {retry_count} in {wait_secs}s...");
        sleep(Duration::from_secs(wait_secs)).await;
    }
}

// 发送消息到指定服务器
//
// 将消息通过上行通道发送到指定服务器的 WebSocket 连接
//
// # 参数
// * `server_name` - 服务器名称
// * `msg` - 要发送的消息
//
// # 返回值
// 成功时返回 Ok(())，失败时返回错误信息
pub async fn send_to(server_name: &str, msg: Message) -> Result<()> {
    let pool = CONNECTION_POOL
        .get()
        .ok_or_else(|| NodegetError::Other("Connection pool not initialized".to_owned()))?;

    let pool_guard = pool.read().await;

    pool_guard.get(server_name).map_or_else(
        || {
            Err(NodegetError::Other(format!(
                "Server not found: {server_name}"
            )))
        },
        |handle| {
            handle
                .uplink_tx
                .send(msg)
                .map(|_| ())
                .map_err(|_| NodegetError::Other("Sending channel issue".to_owned()))
        },
    )
}

// 订阅来自指定服务器的消息
//
// 获取指定服务器下行消息通道的接收器，用于接收来自服务器的消息
//
// # 参数
// * `server_name` - 服务器名称
//
// # 返回值
// 成功时返回消息接收器，失败时返回错误信息
pub async fn subscribe_to(server_name: &str) -> Result<broadcast::Receiver<Message>> {
    let pool = CONNECTION_POOL
        .get()
        .ok_or_else(|| NodegetError::Other("Connection pool not initialized".to_owned()))?;

    let pool_guard = pool.read().await;

    pool_guard.get(server_name).map_or_else(
        || {
            Err(NodegetError::Other(format!(
                "Server not found: {server_name}"
            )))
        },
        |handle| Ok(handle.downlink_tx.subscribe()),
    )
}
