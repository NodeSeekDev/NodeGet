use crate::AGENT_CONFIG;
use crate::rpc::multi_server::{send_to, subscribe_to};
use crate::rpc::{JsonRpcTask, wrap_json_into_rpc_with_id_1};
use log::{debug, error, info, trace};
use nodeget_lib::task::{TaskEventResponse, TaskEventResult, TaskEventType};
use nodeget_lib::utils::get_local_timestamp_ms;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

mod execute;
mod ip;
pub mod ping;
mod pty;

pub async fn handle_task() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    for server in agent_config.server.clone().unwrap_or(vec![]) {
        tokio::spawn(async move {
            if !server.allow_task.unwrap_or(false) {
                return;
            }
            let mut rx = match subscribe_to(server.name.as_str()).await {
                Ok(rx) => {
                    info!("[{}] Handle Task Started", server.name);
                    rx
                }
                Err(e) => {
                    error!("[{}] Handle Task Error: {}", server.name, e);
                    return;
                }
            };

            while let Ok(message) = rx.recv().await {
                let server_name = server.name.clone();
                let server_token = server.token.clone();
                tokio::spawn(async move {
                    let rpc = match message {
                        Message::Text(text) => text.to_string(),
                        _ => {
                            return;
                        }
                    };

                    let json_rpc: JsonRpcTask = match serde_json::from_str(&rpc) {
                        Ok(json_rpc) => json_rpc,
                        Err(_) => {
                            return;
                        }
                    };

                    if json_rpc.method != "task_register_task" {
                        return;
                    }

                    let task_result: Result<TaskEventResult, String> =
                        match json_rpc.params.result.task_event_type {
                            TaskEventType::Ping(target) => {
                                if server.allow_icmp_ping.unwrap_or(false) {
                                    match ping::icmp::ping_target(target).await {
                                        Ok(duration) => {
                                            Ok(TaskEventResult::Ping(duration.as_millis_f64()))
                                        }
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                            TaskEventType::TcpPing(target) => {
                                if server.allow_tcp_ping.unwrap_or(false) {
                                    match ping::tcp::tcping_target(target).await {
                                        Ok(duration) => {
                                            Ok(TaskEventResult::Ping(duration.as_millis_f64()))
                                        }
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                            TaskEventType::HttpPing(target) => {
                                if server.allow_http_ping.unwrap_or(false) {
                                    match ping::http::httping_target(target).await {
                                        Ok(duration) => {
                                            Ok(TaskEventResult::Ping(duration.as_millis_f64()))
                                        }
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                            TaskEventType::WebShell(url) => {
                                if server.allow_web_shell.unwrap_or(false) {
                                    let task_id = json_rpc.params.result.task_id;

                                    let url = pty::parse_url(
                                        url,
                                        task_id,
                                        &json_rpc.params.result.task_token,
                                    );

                                    match pty::handle_pty_url(url).await {
                                        Ok(()) => Ok(TaskEventResult::WebShell(true)),
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                            TaskEventType::Execute(command) => {
                                if server.allow_execute.unwrap_or(false) {
                                    match execute::execute_command(command).await {
                                        Ok(output) => Ok(TaskEventResult::Execute(output)),
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                            TaskEventType::Ip => {
                                if server.allow_ip.unwrap_or(false) {
                                    let ip_info = ip::ip().await;
                                    Ok(TaskEventResult::Ip(ip_info.ipv4, ip_info.ipv6))
                                } else {
                                    Err("102: Permission Denied".to_string())
                                }
                            }
                        };

                    let response = match task_result {
                        Ok(task_result) => TaskEventResponse {
                            task_id: json_rpc.params.result.task_id,
                            agent_uuid: AGENT_CONFIG.get().unwrap().agent_uuid,
                            task_token: json_rpc.params.result.task_token,
                            timestamp: get_local_timestamp_ms(),
                            success: true,
                            error_message: None,
                            task_event_result: Some(task_result),
                        },
                        Err(e) => TaskEventResponse {
                            task_id: json_rpc.params.result.task_id,
                            agent_uuid: AGENT_CONFIG.get().unwrap().agent_uuid,
                            task_token: json_rpc.params.result.task_token,
                            timestamp: get_local_timestamp_ms(),
                            success: false,
                            error_message: Some(e),
                            task_event_result: None,
                        },
                    };

                    let rpc = wrap_json_into_rpc_with_id_1(
                        "task_upload_task_result",
                        vec![
                            serde_json::to_value(server_token).unwrap(),
                            serde_json::to_value(response).unwrap(),
                        ],
                    );

                    if let Err(e) = send_to(&server_name, Message::Text(Utf8Bytes::from(rpc))).await
                    {
                        error!("{e}");
                    }
                });
            }
        });
    }
}
