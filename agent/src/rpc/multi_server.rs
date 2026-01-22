use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::AGENT_CONFIG;
use crate::rpc::wrap_json_into_rpc_with_id_1;
use futures::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use nodeget_lib::config::agent::Server;
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{OnceCell, RwLock, broadcast};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

// 句柄
pub struct ServerHandle {
    uplink_tx: broadcast::Sender<Message>,
    downlink_tx: broadcast::Sender<Message>,
}

// 全局连接池
static CONNECTION_POOL: OnceCell<RwLock<HashMap<String, Arc<ServerHandle>>>> =
    OnceCell::const_new();

pub fn init_connections(servers: Vec<Server>) {
    let mut map = HashMap::new();

    for server in servers {
        let (uplink_tx, uplink_rx) = broadcast::channel::<Message>(32);

        let (downlink_tx, _) = broadcast::channel::<Message>(32);

        let handle = ServerHandle {
            uplink_tx,
            downlink_tx: downlink_tx.clone(),
        };

        map.insert(server.name.clone(), Arc::new(handle));

        tokio::spawn(connection_manager(server, uplink_rx, downlink_tx));
    }

    if CONNECTION_POOL.set(RwLock::new(map)).is_err() {
        warn!("Connection pool has already been initialized");
    } else {
        info!("Connection pool initialized");
    }
}

/// 连接生命周期维护
async fn connection_manager(
    server: Server,
    mut uplink_rx: broadcast::Receiver<Message>,
    downlink_tx: broadcast::Sender<Message>,
) {
    let name = &server.name;
    let url = &server.ws_url;

    info!("[{name}] Manager task started");

    loop {
        info!("[{name}] Connecting to {url}...");

        let Ok(ws_stream) = connect_with_retry(name, url).await else {
            sleep(Duration::from_secs(5)).await;
            continue;
        };

        info!("[{name}] Connected successfully");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Task Register
        {
            if server.allow_task.unwrap_or(false) {
                let rpc = wrap_json_into_rpc_with_id_1(
                    "task_register_task",
                    vec![serde_json::Value::String(
                        AGENT_CONFIG.get().unwrap().agent_uuid.to_string(),
                    )],
                );

                if let Err(e) = ws_write.send(Message::Text(Utf8Bytes::from(rpc))).await {
                    error!(
                        "[{name}] Write error (register task listener): {e}, triggering reconnect..."
                    );
                    continue;
                }
                debug!("[{name}] Register task listener successfully");
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
                            continue;
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

async fn connect_with_retry(
    name: &str,
    url: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, ()> {
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
        let wait_secs = if retry_count < 5 { 2 } else { 5 };
        debug!("[{name}] Retry attempt {retry_count} in {wait_secs}s...");
        sleep(Duration::from_secs(wait_secs)).await;
    }
}

pub async fn send_to(server_name: &str, msg: Message) -> Result<(), String> {
    let pool = CONNECTION_POOL
        .get()
        .ok_or("Connection pool not initialized")?
        .read()
        .await;

    if let Some(handle) = pool.get(server_name) {
        handle
            .uplink_tx
            .send(msg)
            .map(|_| ())
            .map_err(|_| "Sending channel issue".to_string())
    } else {
        Err(format!("Server not found: {server_name}"))
    }
}

pub async fn subscribe_to(server_name: &str) -> Result<broadcast::Receiver<Message>, String> {
    let pool = CONNECTION_POOL
        .get()
        .ok_or("Connection pool not initialized")?
        .read()
        .await;

    if let Some(handle) = pool.get(server_name) {
        Ok(handle.downlink_tx.subscribe())
    } else {
        Err(format!("Server not found: {server_name}"))
    }
}
