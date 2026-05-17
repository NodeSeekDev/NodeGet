use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::rpc::wrap_json_into_rpc_with_id_1;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use nodeget_lib::config::agent::Server;
use nodeget_lib::error::NodegetError;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{OnceCell, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

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
// # 调用契约
//
// 本函数并不会 `abort` 任何已有的 `connection_manager` 任务。重复调用
// （例如 hot-reload 路径）**必须**由调用方先对上一次 `init_connections`
// 返回的 `JoinHandle` 执行 `abort`，否则新旧 manager 会并存一小段时间：
// 旧的 `ServerHandle` 在 `*guard = map` 时被 drop，`uplink_tx` 的 Sender
// 随之 drop，`uplink_rx.recv()` 最终返回 `Closed` 让老 manager 退出，
// 但在此之前老 manager 仍可能重新连接并向服务器上报数据。
//
// `agent/src/main.rs` 当前实现满足此契约（每轮 reload 先调
// `abort_handles` 再调 `init_connections`），但新的调用方必须遵守同样
// 的顺序。
//
// # 参数
// * `servers` - 服务器配置向量
pub async fn init_connections(
    servers: Vec<Server>,
    connect_timeout: Duration,
) -> Vec<JoinHandle<()>> {
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
            connect_timeout,
        )));
    }

    if let Some(pool) = CONNECTION_POOL.get() {
        let mut guard = pool.write().await;
        // 显式 take 出旧 map 后再 drop，确保旧 `ServerHandle` 的
        // `uplink_tx` Sender 在本函数返回前就已释放，从而让任何仍在运行
        // 的老 manager `uplink_rx.recv()` 尽快收到 `Closed`。这是给未
        // 履行上述"先 abort"契约的调用方的一层纵深防御。
        let old_map = std::mem::replace(&mut *guard, map);
        drop(old_map);
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
    connect_timeout: Duration,
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

        let ws_stream = match connect_with_retry(
            name,
            url,
            connect_timeout,
            server.ignore_cert.unwrap_or(false),
        )
        .await
        {
            Ok(ws) => ws,
            Err(e) => {
                error!("[{name}] Failed to connect: {e}");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        info!("[{name}] Connected successfully");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // 校验 Server UUID，区分"网络错误"与"UUID 不匹配"两种情形
        // (review_agent.md #71)。
        match verify_server_uuid(name, &server.server_uuid, &mut ws_write, &mut ws_read).await {
            UuidVerification::Ok => {}
            UuidVerification::Transport(reason) => {
                // 网络层问题：写失败、读超时、连接早关。触发 connect_with_retry
                // 的指数退避即可，不需要额外长 sleep。
                error!("[{name}] Server UUID check transport error: {reason}. reconnecting...");
                continue;
            }
            UuidVerification::Mismatch { expected, got } => {
                // 对端身份错误。多半是 url 配错 / 反代到错了集群；短时间内狂连意义不大，
                // 继续用旧的 30s 冷却防止刷屏。
                error!(
                    "[{name}] Server UUID mismatch: expected '{expected}', got '{got}'. Waiting 30s before retry."
                );
                sleep(Duration::from_secs(30)).await;
                continue;
            }
        }

        // 任务注册
        {
            if server.allow_task.unwrap_or(false) {
                let rpc = wrap_json_into_rpc_with_id_1(
                    "task_register_task",
                    vec![
                        serde_json::Value::String(token.clone()),
                        serde_json::Value::String(crate::config_access::current_agent_uuid_string()),
                    ],
                );

                if let Err(e) = ws_write.send(Message::Text(Utf8Bytes::from(rpc))).await {
                    error!(
                        "[{name}] Write error (register task listener): {e}, triggering reconnect..."
                    );
                    continue;
                }

                match timeout(Duration::from_secs(5), ws_read.next()).await {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        // 显式解析 JSON-RPC 响应：只在 id 与请求匹配、没有 error 且有 result
                        // 时才视为注册成功。任何不符合预期的响应都触发重连，避免把错位消息
                        // 当成 ack 吞掉。
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(v) => {
                                let id_ok =
                                    v.get("id").and_then(serde_json::Value::as_u64) == Some(1);
                                if !id_ok {
                                    error!(
                                        "[{name}] Task subscription ack id mismatch: {v}, reconnecting..."
                                    );
                                    continue;
                                }
                                if v.get("error").is_some() {
                                    error!(
                                        "[{name}] Task subscription rejected: {v}, reconnecting..."
                                    );
                                    continue;
                                }
                                if v.get("result").is_none() {
                                    error!(
                                        "[{name}] Task subscription ack missing result: {v}, reconnecting..."
                                    );
                                    continue;
                                }
                                info!("[{name}] Task listener registered successfully");
                            }
                            Err(e) => {
                                error!(
                                    "[{name}] Failed to parse task subscription ack: {e}, raw={text}, reconnecting..."
                                );
                                continue;
                            }
                        }
                    }
                    Err(_) => {
                        error!("[{name}] Task subscription timeout, reconnecting...");
                        continue;
                    }
                    Ok(None) => {
                        error!(
                            "[{name}] Connection closed during task subscription, reconnecting..."
                        );
                        continue;
                    }
                    Ok(Some(Err(e))) => {
                        error!(
                            "[{name}] Read error during task subscription: {e}, reconnecting..."
                        );
                        continue;
                    }
                    Ok(Some(Ok(_))) => {
                        debug!("[{name}] Non-text message during subscription ack");
                    }
                }
            }
        }

        let mut task_resubscribe_interval = if server.allow_task.unwrap_or(false) {
            Some(tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_mins(1),
                Duration::from_mins(1),
            ))
        } else {
            None
        };

        loop {
            tokio::select! {
                // Channel -> WebSocket (上行数据)
                msg_res = uplink_rx.recv() => {
                    match msg_res {
                        Ok(msg) => {
                            if let Err(e) = ws_write.send(msg).await {
                                error!("[{name}] Write error: {e}, triggering reconnect...");
                                break;
                            }
                        }
                        Err(RecvError::Lagged(skipped_count)) => {
                            warn!("[{name}] Connection lagged, dropped {skipped_count} old messages.");
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
                            if downlink_tx.send(msg).is_err() {
                                warn!("[{name}] Downlink send skipped (no active receivers)");
                            }
                        }
                        Some(Err(e)) => {
                            error!("[{name}] Read error: {e}, reconnecting...");
                            break;
                        }
                        None => {
                            warn!("[{name}] Server closed connection, reconnecting...");
                            break;
                        }
                    }
                }

                // 定时重注册 task（仅 allow_task 时）
                () = async {
                if let Some(ref mut interval) = task_resubscribe_interval {
                    interval.tick().await;
                } else {
                    loop { tokio::time::sleep(Duration::from_hours(1)).await; }
                }
                } => {
                    let rpc = wrap_json_into_rpc_with_id_1(
                        "task_register_task",
                        vec![
                            serde_json::Value::String(token.clone()),
                            serde_json::Value::String(
                                crate::config_access::current_agent_uuid_string(),
                            ),
                        ],
                    );
                    if let Err(e) = ws_write.send(Message::Text(Utf8Bytes::from(rpc))).await {
                        error!("[{name}] Write error on task re-sub: {e}, reconnecting...");
                        break;
                    }
                    debug!("[{name}] Task subscription refreshed");
                }
            }
        }

        warn!("[{name}] Disconnected. Waiting 3s before reconnecting...");
        sleep(Duration::from_secs(3)).await;
    }
}

/// 针对单次 WebSocket 握手后 server-uuid 校验的结果。
///
/// 拆出这个 enum 是为了让 caller 能区分"网络问题"（重试即可）与"对端身份错误"
/// （大概率配置/DNS/反向代理错路，狂连无意义），分别走不同的退避。详见
/// `review_agent.md` #71。
enum UuidVerification {
    /// 对端返回的 server uuid 和 agent 侧配置一致，握手通过。
    Ok,
    /// 写失败、读失败、连接早关、读超时、响应格式错等。连带 `{reason}` 仅用于日志。
    Transport(String),
    /// 对端 uuid 与预期不一致；expected 是 agent 配置的值，got 是对端返回的值。
    Mismatch { expected: String, got: String },
}

/// 向刚建立的 WebSocket 发起 `nodeget-server_uuid` 请求并校验响应。
///
/// 注意：这个函数消耗的是 split 后的可变引用，调用方需要在校验通过后继续使用同一对
/// `ws_write`/`ws_read`。我们借走引用而非 `&mut WebSocketStream` 是为了配合后续
/// 收发循环 —— 那里也基于 split 后的 sink / stream。
async fn verify_server_uuid(
    name: &str,
    expected_uuid: &str,
    ws_write: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ws_read: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> UuidVerification {
    let rpc = wrap_json_into_rpc_with_id_1("nodeget-server_uuid", vec![]);
    if let Err(e) = ws_write.send(Message::Text(Utf8Bytes::from(rpc))).await {
        return UuidVerification::Transport(format!("write error: {e}"));
    }

    let remote_uuid = match timeout(Duration::from_secs(5), ws_read.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("result")?.as_str().map(String::from)),
        Ok(Some(Ok(_))) => {
            return UuidVerification::Transport(
                "unexpected non-text frame during uuid check".to_owned(),
            );
        }
        Ok(Some(Err(e))) => return UuidVerification::Transport(format!("read error: {e}")),
        Ok(None) => return UuidVerification::Transport("connection closed".to_owned()),
        Err(_) => return UuidVerification::Transport("response timeout (5s)".to_owned()),
    };

    match remote_uuid {
        Some(got) if got == expected_uuid => {
            debug!("[{name}] Server UUID verified: {got}");
            UuidVerification::Ok
        }
        Some(got) => UuidVerification::Mismatch {
            expected: expected_uuid.to_owned(),
            got,
        },
        None => UuidVerification::Transport("response missing `result` field".to_owned()),
    }
}

// 带重试机制的 WebSocket 连接
//
// 尝试连接到指定的 WebSocket URL，如果失败则进行指数退避重试，无固定重试上限。
//
// # 退避策略
// `wait = clamp(base * 2^(retry-1), base, cap)`，再叠加 ±20% 的随机抖动，
// 以避免多 agent / 多 server 在服务端恢复瞬间集体重连造成雪崩。
//
// # 参数
// * `name` - 服务器名称（用于日志）
// * `url` - WebSocket URL
// * `connect_timeout` - 每次 WebSocket 建连尝试的超时时间
//
// # 返回值
// 建连成功后返回 WebSocket 流；调用方退出任务（如 `JoinHandle::abort` 取消）会终止循环。
async fn connect_with_retry(
    name: &str,
    url: &str,
    connect_timeout: Duration,
    ignore_cert: bool,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    use rand::Rng;

    const BASE_BACKOFF: Duration = Duration::from_secs(1);
    const MAX_BACKOFF: Duration = Duration::from_mins(1);

    let connector = build_connector(ignore_cert);

    let mut retry_count: u32 = 0;
    loop {
        match timeout(
            connect_timeout,
            connect_async_tls_with_config(url, None, false, connector.clone()),
        )
        .await
        {
            Ok(Ok((ws_stream, _))) => return Ok(ws_stream),
            Ok(Err(e)) => {
                warn!("[{name}] Connect failed: {e}");
            }
            Err(_) => {
                warn!(
                    "[{name}] Connect timeout after {} ms",
                    connect_timeout.as_millis()
                );
            }
        }

        retry_count = retry_count.saturating_add(1);

        // 指数退避：base * 2^(retry-1)，截断到 MAX_BACKOFF
        let exp_secs = BASE_BACKOFF.as_secs().saturating_mul(
            1u64.checked_shl(retry_count.saturating_sub(1).min(16))
                .unwrap_or(1),
        );
        let base_wait = Duration::from_secs(exp_secs).min(MAX_BACKOFF);

        // ±20% jitter，避免与其它 agent 同时重连
        let jitter_factor: f64 = rand::rng().random_range(0.8..1.2);
        let wait =
            Duration::from_secs_f64(base_wait.as_secs_f64() * jitter_factor).min(MAX_BACKOFF);

        debug!(
            "[{name}] Retry attempt {retry_count} in {}ms...",
            wait.as_millis()
        );
        sleep(wait).await;
    }
}

// 危险配置：忽略服务端 TLS 证书校验。
// 仅在用户显式配置 `ignore_cert = true` 时使用。
#[derive(Debug)]
struct NoCertificateVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 根据 `ignore_cert` 构建 TLS Connector。
///
/// 如果 `ignore_cert` 为 `true`，返回一个不对服务器证书做任何校验的
/// Rustls Connector；否则返回 `None`，让 `tokio-tungstenite` 使用默认的
/// 系统/webpki 根证书校验。
pub fn build_connector(ignore_cert: bool) -> Option<Connector> {
    if !ignore_cert {
        return None;
    }
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth();
    Some(Connector::Rustls(Arc::new(config)))
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
// # 订阅时序与 broadcast 语义
//
// 返回的 `broadcast::Receiver` **只会看到订阅之后**由 manager 投递到
// `downlink_tx` 的消息。调用方不应依赖历史消息；常见陷阱：
//
// - 若 `connection_manager` 尚未成功连上 server，`downlink_tx` 还没有
//   任何消息，接收方可能长时间 idle，需要自行加超时处理；
// - 若 manager 已经 broadcast 了超过 channel 容量（32）条消息但订阅方
//   尚未订阅，订阅方 `recv()` 会先看到 `RecvError::Lagged(n)`。调用方
//   必须容忍此错误（通常是 `warn!` + `continue`），不要因此退出循环。
//
// 如果需要"订阅即拿到最新快照"的语义，考虑改用
// `tokio::sync::watch` 存最新状态，并把 broadcast 仅用于增量差分。
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
