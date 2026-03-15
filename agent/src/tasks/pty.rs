use crate::AGENT_CONFIG;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use nodeget_lib::error::NodegetError;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::{
    sync::{RwLock, mpsc},
    task,
};
use tokio_tungstenite::tungstenite::Bytes;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::protocol::Message};
use url::Url;

/// PTY result type
pub type Result<T> = std::result::Result<T, NodegetError>;

type TerminalConnectionPool = Arc<RwLock<HashSet<String>>>;

static TERMINAL_CONNECTION_POOL: OnceLock<TerminalConnectionPool> = OnceLock::new();

fn terminal_connection_pool() -> &'static TerminalConnectionPool {
    TERMINAL_CONNECTION_POOL.get_or_init(|| Arc::new(RwLock::new(HashSet::new())))
}

async fn reserve_terminal_id(terminal_id: &str) -> Result<()> {
    let pool = terminal_connection_pool();
    let mut guard = pool.write().await;
    if guard.contains(terminal_id) {
        return Err(NodegetError::InvalidInput(format!(
            "Terminal ID '{terminal_id}' is already connected"
        )));
    }
    guard.insert(terminal_id.to_owned());
    Ok(())
}

async fn release_terminal_id(terminal_id: &str) {
    let pool = terminal_connection_pool();
    let mut guard = pool.write().await;
    guard.remove(terminal_id);
}

// Handle PTY (pseudo terminal) websocket URL.
//
// This function connects to the target websocket URL and starts a PTY session.
//
// # Arguments
// * `url` - websocket URL wrapped in Result
// * `terminal_id` - terminal connection ID
//
// # Returns
// Returns `Ok(())` on success, otherwise an error message.
pub async fn handle_pty_url(
    url: std::result::Result<Url, String>,
    terminal_id: String,
) -> Result<()> {
    let url = match url {
        Ok(url) => url,
        Err(e) => {
            return Err(NodegetError::Other(e));
        }
    };

    reserve_terminal_id(&terminal_id).await?;

    let connect_result = async {
        let Ok(ws) = connect_async(url.to_string()).await else {
            return Err(NodegetError::AgentConnectionError(
                "Failed to connect to WebSocket".to_owned(),
            ));
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
    .await;

    release_terminal_id(&terminal_id).await;

    connect_result
}

// Handle a PTY session.
//
// This function creates a PTY, and forwards websocket messages and PTY IO bidirectionally.
//
// # Arguments
// * `ws_stream` - websocket stream
// * `cmd` - command to run inside PTY
//
// # Returns
// Returns `Ok(())` on success, otherwise an error message.
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

// Terminal resize request payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedResize {
    #[serde(rename = "type")]
    type_str: String, // message type
    cols: u16, // columns
    rows: u16, // rows
}

// Heartbeat payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct HeartBeat {
    #[serde(rename = "type")]
    type_str: String, // message type
    timestamp: String, // timestamp
}

// Handle websocket message.
//
// Depending on message type, this can be heartbeat, resize, or terminal input.
//
// # Arguments
// * `msg` - websocket message
// * `pty_writer` - PTY writer
//
// # Returns
// Returns resize info (if any), otherwise `None`. Returns error on failure.
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

// Parse PTY URL.
//
// Converts an original URL into an effective terminal URL.
// If path is `/auto_gen`, it is replaced with a generated terminal path.
//
// # Arguments
// * `url` - original URL
// * `task_id` - task ID
// * `task_token` - task token
// * `terminal_id` - terminal connection ID
//
// # Returns
// Returns parsed URL on success, or an error message.
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
