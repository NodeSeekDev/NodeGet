use crate::AGENT_CONFIG;
use crate::rpc::multi_server::{send_to, subscribe_to};
use crate::rpc::{JsonRpcTask, wrap_json_into_rpc_with_id_1};
use log::{error, info};
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::{TaskEventResponse, TaskEventResult, TaskEventType};
use nodeget_lib::utils::get_local_timestamp_ms;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

/// Task 结果类型
pub type Result<T> = anyhow::Result<T>;

// 任务执行模块
mod execute;
// IP 获取模块
mod ip;
// Ping 任务模块
pub mod ping;
// PTY（伪终端）模块
mod pty;

// 检查服务器是否允许执行特定任务
fn is_task_allowed(server: &nodeget_lib::config::agent::Server, task_type: &TaskEventType) -> bool {
    match task_type {
        TaskEventType::Ping(_) => server.allow_icmp_ping.unwrap_or(false),
        TaskEventType::TcpPing(_) => server.allow_tcp_ping.unwrap_or(false),
        TaskEventType::HttpPing(_) => server.allow_http_ping.unwrap_or(false),
        TaskEventType::WebShell(_) => server.allow_web_shell.unwrap_or(false),
        TaskEventType::Execute(_) => server.allow_execute.unwrap_or(false),
        TaskEventType::Ip => server.allow_ip.unwrap_or(false),
    }
}

// 执行具体任务
async fn execute_task(
    task_type: &TaskEventType,
    task_id: u64,
    task_token: &str,
) -> Result<TaskEventResult> {
    match task_type {
        TaskEventType::Ping(target) => ping::icmp::ping_target(target.clone())
            .await
            .map(|d| task_type.result_from_duration(d))
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::TcpPing(target) => ping::tcp::tcping_target(target.clone())
            .await
            .map(|d| task_type.result_from_duration(d))
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::HttpPing(target) => ping::http::httping_target(target.clone())
            .await
            .map(|d| task_type.result_from_duration(d))
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::WebShell(web_shell) => {
            let terminal_id = web_shell.terminal_id.to_string();
            let url = pty::parse_url(web_shell.url.clone(), task_id, task_token, &terminal_id);
            pty::handle_pty_url(url, terminal_id)
                .await
                .map(|()| TaskEventResult::WebShell(true))
                .map_err(|e| NodegetError::Other(format!("{e}")).into())
        }

        TaskEventType::Execute(command) => execute::execute_command(command.clone())
            .await
            .map(TaskEventResult::Execute)
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::Ip => {
            let ip_info = ip::ip().await;
            Ok(TaskEventResult::Ip(ip_info.ipv4, ip_info.ipv6))
        }
    }
}

// 处理来自服务器的任务请求
//
// 该函数订阅各个服务器的任务通道，接收并执行不同类型的任务（如 Ping、TCP Ping、HTTP Ping、WebShell、命令执行、IP 查询），
// 然后将执行结果返回给服务器
pub async fn handle_task() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    for server in agent_config.server.clone().unwrap_or(vec![]) {
        tokio::spawn(async move {
            if !server.allow_task.unwrap_or(false) {
                return;
            }
            let mut rx: tokio::sync::broadcast::Receiver<Message> =
                match subscribe_to(server.name.as_str()).await {
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
                let server_config = server.clone();
                tokio::spawn(async move {
                    let rpc = match message {
                        Message::Text(text) => text.to_string(),
                        _ => return,
                    };

                    let json_rpc: JsonRpcTask = match serde_json::from_str(&rpc) {
                        Ok(json_rpc) => json_rpc,
                        Err(_) => return,
                    };

                    if json_rpc.method != "task_register_task" {
                        return;
                    }

                    let task_type = &json_rpc.params.result.task_event_type;

                    let task_result: Result<TaskEventResult> =
                        if is_task_allowed(&server_config, task_type) {
                            execute_task(
                                task_type,
                                json_rpc.params.result.task_id,
                                &json_rpc.params.result.task_token,
                            )
                            .await
                        } else {
                            Err(NodegetError::PermissionDenied(
                                "Permission Denied: Task not allowed".to_owned(),
                            )
                            .into())
                        };

                    let timestamp = get_local_timestamp_ms().unwrap_or(0);

                    let response = match task_result {
                        Ok(task_result) => TaskEventResponse {
                            task_id: json_rpc.params.result.task_id,
                            agent_uuid: AGENT_CONFIG.get().unwrap().agent_uuid,
                            task_token: json_rpc.params.result.task_token,
                            timestamp,
                            success: true,
                            error_message: None,
                            task_event_result: Some(task_result),
                        },
                        Err(e) => {
                            let error_message = format!("{e}");
                            TaskEventResponse {
                                task_id: json_rpc.params.result.task_id,
                                agent_uuid: AGENT_CONFIG.get().unwrap().agent_uuid,
                                task_token: json_rpc.params.result.task_token,
                                timestamp,
                                success: false,
                                error_message: Some(error_message),
                                task_event_result: None,
                            }
                        }
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
