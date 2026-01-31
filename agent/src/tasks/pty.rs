use std::fs;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, task};
use tokio_tungstenite::tungstenite::Bytes;
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message, connect_async};
use url::Url;
use crate::AGENT_CONFIG;

pub async fn handle_pty_url(url: Result<Url, String>) -> Result<(), String> {
    let url = match url {
        Ok(url) => {url},
        Err(e) => {
            return Err(e);
        }
    };

    let ws = match connect_async(url.to_string()).await {
        Ok(ws) => {ws}
        Err(_) => {
            return Err(String::from("Failed to connect to WebSocket"));
        }
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

async fn handle_pty_session<S>(ws_stream: WebSocketStream<S>, cmd: &str) -> Result<(), String>
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
        .map_err(|e| format!("Failed to create PTY: {e}"))?;

    let mut cmd = CommandBuilder::new(cmd);

    if !cfg!(windows) {
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "C.UTF-8");
        cmd.env("LC_ALL", "C.UTF-8");
    }

    let mut pty_reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to get PTY Reader: {e}"))?;
    let pty_writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|e| format!("Failed to get PTY Writer: {e}"))?,
    ));

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn process: {e}"))?;

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
        .map_err(|e| format!("Failed to wait for child process: {e}"))?;
    info!("Session successfully closed.");

    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedResize {
    #[serde(rename = "type")]
    type_str: String,
    cols: u16,
    rows: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct HeartBeat {
    #[serde(rename = "type")]
    type_str: String,
    timestamp: String,
}

fn handle_ws_message(
    msg: Message,
    pty_writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) -> Result<Option<NeedResize>, String> {

    match msg {
        Message::Text(text) => {
            if serde_json::from_str::<HeartBeat>(text.as_ref()).is_ok()  {
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

pub fn parse_url(url: Url, task_id: u64, task_token: String) -> Result<Url, String> {
    let scheme = url.scheme();
    if !((scheme == "ws") || (scheme == "wss")) {
        return Err(format!("Invalid scheme: {scheme}"));
    }

    let url = if url.path() == "/auto_gen" {
        let agent_uuid = AGENT_CONFIG.get().ok_or("Agent Config 未初始化")?.agent_uuid;
        let host = url.host_str().ok_or(format!("Invalid host: {}", url))?;
        let port = url.port_or_known_default().ok_or(format!("Invalid port: {}", url))?;

        let url = format!("{}://{}:{}/terminal?agent_uuid={}&task_id={}&task_token={}", scheme, host, port, agent_uuid, task_id, task_token);
        Url::parse(&url).map_err(|e| format!("Invalid URL: {e}"))?
    } else {
        url
    };
Ok(url)
}