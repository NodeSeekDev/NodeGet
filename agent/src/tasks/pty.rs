use crate::AGENT_CONFIG;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use nodeget_lib::error::NodegetError;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, task};
use tokio_tungstenite::tungstenite::Bytes;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::protocol::Message};
use url::Url;

/// PTY 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// 处理 PTY（伪终端）URL
//
// 该函数连接到指定的 WebSocket URL，并启动 PTY 会话
//
// # 参数
// * `url` - WebSocket URL，包装在 Result 中
//
// # 返回值
// 成功时返回 Ok(())，失败时返回错误信息
pub async fn handle_pty_url(url: std::result::Result<Url, String>) -> Result<()> {
    let url = match url {
        Ok(url) => url,
        Err(e) => {
            return Err(NodegetError::Other(e));
        }
    };

    let Ok(ws) = connect_async(url.to_string()).await else {
        return Err(NodegetError::AgentConnectionError("Failed to connect to WebSocket".to_owned()));
    };

    let ws_stream = ws.0;

    let cmd = if cfg!(windows) {
        "cmd.exe"
    } else if fs::exists("/bin/bash").unwrap_or(false) {
        "bash"
    } else {
        "sh"
    };

    handle_pty_session(ws_stream, cmd).await
}

// 处理 PTY 会话
//
// 该函数创建一个 PTY（伪终端），并将 WebSocket 消息与 PTY 输入输出进行双向转发
//
// # 参数
// * `ws_stream` - WebSocket 流
// * `cmd` - 要在 PTY 中运行的命令
//
// # 返回值
// 成功时返回 Ok(())，失败时返回错误信息
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
    }

    let mut pty_reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| NodegetError::Other(format!("Failed to get PTY Reader: {e}")))?;
    let pty_writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|e| NodegetError::Other(format!("Failed to get PTY Writer: {e}")))?,
    ));

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| NodegetError::Other(format!("Failed to spawn process: {e}")))?;

    info!("Terminal started in PTY, PID: {:?}", child.process_id());

    let (ws_sender, mut ws_receiver) = ws_stream.split();
    let (pty_to_ws_tx, mut pty_to_ws_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    task::spawn_blocking(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match pty_reader.read(&mut buffer) {
                Ok(count) if count > 0 => {
                    if pty_to_ws_tx.send(buffer[..count].to_vec()).is_err() {
                        info!("PTY reader: WebSocket side closed, stopping read.");
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

    let pty_to_ws_task = tokio::spawn(async move {
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

    let ws_to_pty_task = tokio::spawn(async move {
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
        _ = pty_to_ws_task => info!("PTY -> WebSocket task finished."),
        _ = ws_to_pty_task => info!("WebSocket -> PTY task finished."),
    }

    info!("Closing session, terminating child process...");
    if let Err(e) = child.kill() {
        error!("Failed to terminate child process: {e}");
    }
    child
        .wait()
        .map_err(|e| NodegetError::Other(format!("Failed to wait for child process: {e}")))?;
    info!("Session successfully closed.");

    Ok(())
}

// 终端调整大小请求结构体
#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedResize {
    #[serde(rename = "type")]
    type_str: String, // 消息类型
    cols: u16, // 列数
    rows: u16, // 行数
}

// 心跳消息结构体
#[derive(Serialize, Deserialize, Debug, Clone)]
struct HeartBeat {
    #[serde(rename = "type")]
    type_str: String, // 消息类型
    timestamp: String, // 时间戳
}

// 处理 WebSocket 消息
//
// 该函数根据消息类型决定如何处理 WebSocket 消息，可能是心跳、调整大小或终端输入
//
// # 参数
// * `msg` - WebSocket 消息
// * `pty_writer` - PTY 写入器
//
// # 返回值
// 成功时返回调整大小信息（如果有），失败时返回错误信息
fn handle_ws_message(
    msg: Message,
    pty_writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) -> std::result::Result<Option<NeedResize>, String> {
    match msg {
        Message::Text(text) => {
            if serde_json::from_str::<HeartBeat>(text.as_ref()).is_ok() {
                return Ok(None);
            }
            if let Ok(resize) = serde_json::from_str::<NeedResize>(text.as_ref()) {
                return Ok(Some(resize));
            }
            pty_writer
                .lock()
                .unwrap()
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write to PTY: {e}"))?;
        }
        Message::Binary(data) => {
            pty_writer
                .lock()
                .unwrap()
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

// 解析 PTY URL
//
// 该函数将原始 URL 转换为有效的终端连接 URL，如果路径是 "/auto_gen" 则替换为实际的终端连接路径
//
// # 参数
// * `url` - 原始 URL
// * `task_id` - 任务 ID
// * `task_token` - 任务令牌
//
// # 返回值
// 成功时返回解析后的 URL，失败时返回错误信息
pub fn parse_url(url: Url, task_id: u64, task_token: &str) -> std::result::Result<Url, String> {
    let scheme = url.scheme();
    if !((scheme == "ws") || (scheme == "wss")) {
        return Err(format!("Invalid scheme: {scheme}"));
    }

    let url = if url.path() == "/auto_gen" {
        let agent_uuid = AGENT_CONFIG
            .get()
            .ok_or("Agent Config 未初始化")?
            .agent_uuid;
        let host = url
            .host_str()
            .ok_or_else(|| format!("Invalid host: {url}"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("Invalid port: {url}"))?;

        let url = format!(
            "{scheme}://{host}:{port}/terminal?agent_uuid={agent_uuid}&task_id={task_id}&task_token={task_token}"
        );
        Url::parse(&url).map_err(|e| format!("Invalid URL: {e}"))?
    } else {
        url
    };
    Ok(url)
}
