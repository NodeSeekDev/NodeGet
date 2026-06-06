//! PTY（伪终端）WebShell 任务模块。
//!
//! 建立 Agent 到 Server 的 WebSocket 连接，创建本地 PTY 会话，
//! 双向转发 WebSocket 消息与 PTY IO。支持终端尺寸调整和心跳协议。

use crate::AGENT_CONFIG;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use ng_core::error::NodegetError;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::{
    sync::{RwLock, mpsc},
    task,
};
use tokio_tungstenite::tungstenite::Bytes;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{WebSocketStream, connect_async_tls_with_config};
use url::Url;

/// PTY 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

/// 终端连接池类型，用于防止同一 `terminal_id` 重复连接。
type TerminalConnectionPool = Arc<RwLock<HashSet<String>>>;

/// 全局终端连接池，跟踪当前活跃的 `terminal_id`，防止重复连接。
static TERMINAL_CONNECTION_POOL: LazyLock<TerminalConnectionPool> =
    LazyLock::new(|| Arc::new(RwLock::new(HashSet::new())));

/// 预留 `terminal_id`，防止同一终端 ID 被并发连接。
///
/// - `terminal_id` - 终端连接 ID
///
/// 返回 `Ok(())`；ID 已存在时返回错误。
async fn reserve_terminal_id(terminal_id: &str) -> Result<()> {
    let mut guard = TERMINAL_CONNECTION_POOL.write().await;
    if guard.contains(terminal_id) {
        return Err(NodegetError::InvalidInput(format!(
            "Terminal ID '{terminal_id}' is already connected"
        )));
    }
    guard.insert(terminal_id.to_owned());
    Ok(())
}

/// 释放 `terminal_id`，从连接池中移除。
///
/// - `terminal_id` - 终端连接 ID
async fn release_terminal_id(terminal_id: &str) {
    let mut guard = TERMINAL_CONNECTION_POOL.write().await;
    guard.remove(terminal_id);
}

/// RAII 守卫：drop 时自动释放 `terminal_id`，防止热重载 abort 等场景下 ID 泄漏。
struct TerminalIdGuard {
    id: String,
    released: bool,
}

impl TerminalIdGuard {
    /// 创建守卫并预留 `terminal_id`。
    async fn reserve(terminal_id: String) -> Result<Self> {
        reserve_terminal_id(&terminal_id).await?;
        Ok(Self {
            id: terminal_id,
            released: false,
        })
    }

    /// 提前释放（正常退出路径），避免等待 Drop 时再获取写锁。
    async fn release(mut self) {
        self.released = true;
        release_terminal_id(&self.id).await;
    }
}

impl Drop for TerminalIdGuard {
    fn drop(&mut self) {
        if !self.released {
            // Drop 中无法 await，用 block_on 同步释放；仅在 abort 等异常路径触发
            let id = self.id.clone();
            // 仅在未释放时执行，且通过新 tokio runtime 避免嵌套 panic
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.block_on(async { release_terminal_id(&id).await });
            }
        }
    }
}

/// 处理 PTY WebSocket URL，建立连接并启动终端会话。
///
/// - `url` - WebSocket URL（可能包含解析错误）
/// - `terminal_id` - 终端连接 ID
/// - `ignore_cert` - 是否跳过 TLS 证书校验
///
/// 1. 预留 `terminal_id`（防止重复连接）
/// 2. 建立 WebSocket 连接（带 10 秒超时）
/// 3. 获取终端 shell 路径
/// 4. 启动 PTY 会话
/// 5. 会话结束后释放 `terminal_id`
///
/// 返回 `Ok(())`；连接失败或 PTY 错误时返回错误。
pub async fn handle_pty_url(
    url: std::result::Result<Url, String>,
    terminal_id: String,
    ignore_cert: bool,
) -> Result<()> {
    let url = match url {
        Ok(url) => url,
        Err(e) => {
            return Err(NodegetError::Other(e));
        }
    };

    let guard = TerminalIdGuard::reserve(terminal_id).await?;

    let connect_result = async {
        // 限制 connect_async 最多 10s 握手，避免恶意/异常 server 让任务挂死，
        // 同时配合 `release_terminal_id` 保证 terminal_id 不会被永远占用。
        const PTY_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
        let connector = crate::rpc::multi_server::build_connector(ignore_cert);
        let ws = match tokio::time::timeout(
            PTY_CONNECT_TIMEOUT,
            connect_async_tls_with_config(url.to_string(), None, false, connector),
        )
        .await
        {
            Ok(Ok(ws)) => ws,
            Ok(Err(e)) => {
                return Err(NodegetError::AgentConnectionError(format!(
                    "Failed to connect to WebSocket: {e}"
                )));
            }
            Err(_) => {
                return Err(NodegetError::AgentConnectionError(format!(
                    "WebSocket connect timed out after {}s",
                    PTY_CONNECT_TIMEOUT.as_secs()
                )));
            }
        };

        let ws_stream = ws.0;

        let cmd = terminal_shell()?;

        handle_pty_session(ws_stream, &cmd).await
    }
    .await;

    guard.release().await;

    connect_result
}

/// 获取终端 shell 路径。
///
/// 优先使用配置中的 `terminal_shell`，未配置或为空时回退到默认值
///（Windows: `cmd.exe`，有 `/bin/bash` 则用 bash，否则 `sh`）。
/// 配置的路径若包含多级目录且不存在，返回错误。
fn terminal_shell() -> Result<String> {
    let Some(config) = AGENT_CONFIG.get() else {
        return Err(NodegetError::Other(
            "Agent config not initialized".to_owned(),
        ));
    };

    let configured_shell = config
        .read()
        .map_err(|_| NodegetError::Other("AGENT_CONFIG lock poisoned".to_owned()))?
        .terminal_shell
        .clone();

    let shell = configured_shell.map_or_else(
        || default_terminal_shell().to_owned(),
        |shell| {
            let shell = shell.trim();
            if shell.is_empty() {
                default_terminal_shell().to_owned()
            } else {
                shell.to_owned()
            }
        },
    );

    let shell_path = Path::new(&shell);
    if shell_path.components().count() > 1 && !shell_path.exists() {
        return Err(NodegetError::InvalidInput(format!(
            "Configured terminal_shell does not exist: {shell}"
        )));
    }

    Ok(shell)
}

/// 返回平台默认终端 shell 路径。
fn default_terminal_shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else if Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else {
        "sh"
    }
}

/// 处理 PTY 会话，双向转发 WebSocket 消息与 PTY IO。
///
/// - `ws_stream` - WebSocket 流
/// - `cmd` - 在 PTY 中运行的命令
///
/// 1. 创建 PTY 并设置初始尺寸（24x80）
/// 2. 配置子进程环境变量（TERM、LANG、PATH、HOME、USER）
/// 3. 启动三个并发任务：PTY→WebSocket、WebSocket→PTY、PTY 读取
/// 4. 任一方向结束后 abort 对端任务
/// 5. 整组 SIGTERM → 200ms 等待 → SIGKILL 终止子进程组
///
/// 返回 `Ok(())`；PTY 或 WebSocket 错误时返回错误。
async fn handle_pty_session<S>(ws_stream: WebSocketStream<S>, cmd: &str) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let pty_system = NativePtySystem::default();

    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| NodegetError::Other(format!("Failed to create PTY: {e}")))?;

    let mut cmd = CommandBuilder::new(cmd);

    if !cfg!(windows) {
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "C.UTF-8");
        cmd.env("LC_ALL", "C.UTF-8");
        // 显式透传 PATH / HOME / USER。portable_pty 默认会把父进程 env 注入到子进程，
        // 但这个"默认"并不在公开 API 合同里（依赖 CommandBuilder 内部行为）；显式透传
        // 能保证即使 nodeget-agent 作为 systemd service 运行（environment 只有 PATH=/usr/sbin:/usr/bin）
        // 时，bash 里仍然能找到 `ls`、`cd`、`git` 等常用命令。HOME 与 USER 同样被多数 shell 启
        // 动脚本（~/.bashrc、/etc/profile.d/*）依赖。
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
        if let Ok(user) = std::env::var("USER") {
            cmd.env("USER", user);
        }
    }

    let mut pty_reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| NodegetError::Other(format!("Failed to get PTY Reader: {e}")))?;
    let pty_writer =
        Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| {
            NodegetError::Other(format!("Failed to get PTY Writer: {e}"))
        })?));

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| NodegetError::Other(format!("Failed to spawn process: {e}")))?;

    info!("Terminal started in PTY, PID: {:?}", child.process_id());

    let (ws_sender, mut ws_receiver) = ws_stream.split();
    // PTY→WS 使用有界通道，网络拥塞时对 PTY 读取施加背压而非无限堆积
    let (pty_to_ws_tx, mut pty_to_ws_rx) = mpsc::channel::<Vec<u8>>(4096);

    task::spawn_blocking(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match pty_reader.read(&mut buffer) {
                Ok(count) if count > 0 => {
                    // try_send 非阻塞：通道满时丢弃数据而非阻塞 spawn_blocking 线程，
                    // 避免 PTY 读取线程卡死导致整个会话僵死
                    if pty_to_ws_tx.try_send(buffer[..count].to_vec()).is_err() {
                        info!("PTY reader: channel full or closed, stopping read.");
                        break;
                    }
                }
                Ok(_) | Err(_) => {
                    info!("PTY reader: PTY closed, stopping read.");
                    break;
                }
            }
        }
    });

    let mut pty_to_ws_task = tokio::spawn(async move {
        let mut ws_sender = ws_sender;
        while let Some(data) = pty_to_ws_rx.recv().await {
            if ws_sender
                .send(Message::Binary(Bytes::from(data)))
                .await
                .is_err()
            {
                error!("Failed to send data to WebSocket");
                break;
            }
        }
    });

    let mut ws_to_pty_task = tokio::spawn(async move {
        while let Some(result) = ws_receiver.next().await {
            match result {
                Ok(msg) => match handle_ws_message(msg, &pty_writer) {
                    Err(e) => {
                        error!("Failed to handle WebSocket message: {e}");
                        break;
                    }
                    Ok(Some(resize)) => {
                        if let Err(e) = pair.master.resize(PtySize {
                            rows: resize.rows,
                            cols: resize.cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        }) {
                            error!("Failed to resize PTY: {e}");
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    error!("Error receiving message from WebSocket: {e}");
                    break;
                }
            }
        }
    });

    tokio::select! {
        _ = &mut pty_to_ws_task => {
            info!("PTY -> WebSocket task finished.");
            // 另一边可能仍在 `ws_receiver.next()` 里等待，主动 abort 防止 session 结束后
            // 仍有悬挂的 ws_to_pty_task 占用 WebSocket 读端。
            ws_to_pty_task.abort();
        }
        _ = &mut ws_to_pty_task => {
            info!("WebSocket -> PTY task finished.");
            // abort pty_to_ws_task 让 `pty_to_ws_rx` 尽早 drop；这样 spawn_blocking
            // 的 reader 下次 `pty_to_ws_tx.send` 会返回 Err 并退出 loop，避免
            // "reader task 永不退出" 的泄漏（尤其在 Windows ConPTY `read` 即便
            // slave 关闭后仍可能长时间 block 的场景下）。
            pty_to_ws_task.abort();
        }
    }

    info!("Closing session, terminating child process...");
    // portable_pty 在 Unix 下 fork 时执行了 setsid()，shell 成为进程组
    // 组长（pgid == child pid）。`child.kill()` 只 SIGKILL shell 自己，
    // 会话中由 shell 启动的子进程（tmux、nohup、后台任务等）若自行
    // 脱离父进程，会被 init 认领为孤儿进程。
    //
    // 流程：整组 SIGTERM → 等 200ms → 整组 SIGKILL 兜底 → `child.wait()`。
    // 之前版本注释里提到"短暂等待"但代码直接从 SIGTERM 跳到 `child.kill()`，
    // 实际上没给进程组任何清理时间，这里补齐延迟。非 Unix 平台没有进程组
    // 概念，退化为 `child.kill()`。
    #[cfg(unix)]
    {
        if let Some(pid) = child.process_id() {
            #[allow(clippy::cast_possible_wrap)]
            let pgid = pid as i32;
            // SAFETY: libc::killpg 签名要求 signed pid；整组信号若组已全部退出只是
            // 返回 ESRCH，不会造成未定义行为。
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        } else if let Err(e) = child.kill() {
            error!("Failed to terminate child process: {e}");
        }
    }
    #[cfg(not(unix))]
    if let Err(e) = child.kill() {
        error!("Failed to terminate child process: {e}");
    }
    child
        .wait()
        .map_err(|e| NodegetError::Other(format!("Failed to wait for child process: {e}")))?;
    info!("Session successfully closed.");

    Ok(())
}

/// 终端尺寸调整请求载荷。
#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedResize {
    /// 消息类型
    #[serde(rename = "type")]
    type_str: String,
    /// 列数
    cols: u16,
    /// 行数
    rows: u16,
}

/// 心跳载荷。
#[derive(Serialize, Deserialize, Debug, Clone)]
struct HeartBeat {
    /// 消息类型
    #[serde(rename = "type")]
    type_str: String,
    /// 时间戳
    timestamp: String,
}

/// 处理 WebSocket 消息，分派为终端输入、尺寸调整或心跳。
///
/// - `msg` - WebSocket 消息
/// - `pty_writer` - PTY 写入器
///
/// 先尝试 JSON 解析：有 `cols`/`rows` 字段视为 resize，有 `type`/`timestamp` 视为心跳丢弃；
/// 非 JSON 或无控制字段则作为终端输入写入 PTY。
///
/// 返回 `Ok(Some(NeedResize))` 表示需要调整尺寸；`Ok(None)` 表示无需调整；解析失败返回错误。
fn handle_ws_message(
    msg: Message,
    pty_writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) -> std::result::Result<Option<NeedResize>, String> {
    match msg {
        Message::Text(text) => {
            // 先尝试一次 JSON 解析；若不是 JSON 或缺少控制字段，直接当终端输入写入 PTY。
            // 旧实现对每条文本消息做两次 `from_str`（HeartBeat + NeedResize），终端按键
            // 频率高时开销可观，这里改为一次 Value 解析 + 结构化分派。协议语义保持不变：
            // 有 `cols` / `rows` 字段 → 视为 resize；否则（含 type==heartbeat、无 type）一律丢弃为控制。
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.as_ref())
                && v.is_object()
            {
                // resize：只要结构上匹配 cols/rows 即视为 resize（与旧行为一致）。
                if let (Some(cols), Some(rows)) = (
                    v.get("cols").and_then(serde_json::Value::as_u64),
                    v.get("rows").and_then(serde_json::Value::as_u64),
                ) {
                    return Ok(Some(NeedResize {
                        type_str: v
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("resize")
                            .to_owned(),
                        cols: u16::try_from(cols).unwrap_or(u16::MAX),
                        rows: u16::try_from(rows).unwrap_or(u16::MAX),
                    }));
                }
                // 结构上像 heartbeat：带 type 和 timestamp，或没有终端输入负载但属于控制消息。
                if v.get("type").is_some() && v.get("timestamp").is_some() {
                    return Ok(None);
                }
            }
            pty_writer
                .lock()
                .map_err(|_| "PTY writer mutex poisoned".to_string())?
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write to PTY: {e}"))?;
        }
        Message::Binary(data) => {
            pty_writer
                .lock()
                .map_err(|_| "PTY writer mutex poisoned".to_string())?
                .write_all(&data)
                .map_err(|e| format!("Failed to write to PTY: {e}"))?;
        }
        Message::Close(_) => {
            return Err(String::from("WebSocket connection closed"));
        }
        _ => {}
    }
    Ok(None)
}

/// 解析并构造 PTY WebSocket URL。
///
/// - `url` - 原始 URL
/// - `task_id` - 任务 ID
/// - `task_token` - 任务令牌
/// - `terminal_id` - 终端连接 ID
///
/// 若路径为 `/auto_gen`，自动生成包含 `agent_uuid`、`task_id`、`task_token`、`terminal_id` 的完整 URL；
/// 否则仅追加/替换 `terminal_id` 查询参数。
///
/// 返回解析后的 URL；URL 格式错误时返回错误字符串。
pub fn parse_url(
    url: Url,
    task_id: u64,
    task_token: &str,
    terminal_id: &str,
) -> std::result::Result<Url, String> {
    let scheme = url.scheme();
    if !((scheme == "ws") || (scheme == "wss")) {
        return Err(format!("Invalid scheme: {scheme}"));
    }

    let mut url = if url.path() == "/auto_gen" {
        let agent_uuid = AGENT_CONFIG
            .get()
            .ok_or("Agent config not initialized")?
            .read()
            .map_err(|_| "Agent Config lock poisoned")?
            .agent_uuid;
        let host = url
            .host_str()
            .ok_or_else(|| format!("Invalid host: {url}"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("Invalid port: {url}"))?;

        let url = format!(
            "{scheme}://{host}:{port}/terminal?agent_uuid={agent_uuid}&task_id={task_id}&task_token={task_token}&terminal_id={terminal_id}"
        );
        Url::parse(&url).map_err(|e| format!("Invalid URL: {e}"))?
    } else {
        url
    };

    set_or_replace_query_param(&mut url, "terminal_id", terminal_id);
    Ok(url)
}

/// 设置或替换 URL 中的查询参数。
///
/// - `url` - 可变 URL 引用
/// - `key` - 参数名
/// - `value` - 参数值
fn set_or_replace_query_param(url: &mut Url, key: &str, value: &str) {
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| k != key)
        .collect();

    {
        let mut serializer = url.query_pairs_mut();
        serializer.clear();
        for (k, v) in pairs {
            serializer.append_pair(&k, &v);
        }
        serializer.append_pair(key, value);
    }
}
