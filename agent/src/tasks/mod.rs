use crate::config_access::get_agent_config;
use crate::rpc::multi_server::{send_to, subscribe_to};
use crate::rpc::{JsonRpcTask, wrap_json_into_rpc_with_id_1};
use crate::{AGENT_ARGS, RELOAD_NOTIFY};
use log::{error, info, warn};
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::{TaskEventResponse, TaskEventResult, TaskEventType};
use nodeget_lib::utils::get_local_timestamp_ms;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::{fs, time};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

/// Task 结果类型
pub type Result<T> = anyhow::Result<T>;

/// 单条非 `WebShell` 任务的硬性执行上限。
///
/// 防止单个任务（被劫持 / 意外卡死 / 外部资源不可达）永久占用
/// `handle_task` 中的 per-message `JoinSet` 插槽。10 分钟远大于正常
/// ICMP/TCP/HTTP ping 与 execute / `http_request` 的预期上限，又不
/// 会让真正卡住的任务在 agent 进程里无限堆积。
const TASK_MAX_TIMEOUT: Duration = Duration::from_mins(10);

// 任务执行模块
mod execute;
// IP 获取模块
mod ip;
// HTTP Request 任务模块
mod http_request;
// Ping 任务模块
pub mod ping;
// PTY（伪终端）模块
mod pty;
pub mod self_update;

// 检查服务器是否允许执行特定任务
fn is_task_allowed(server: &nodeget_lib::config::agent::Server, task_type: &TaskEventType) -> bool {
    match task_type {
        TaskEventType::Ping(_) => server.allow_icmp_ping.unwrap_or(false),
        TaskEventType::TcpPing(_) => server.allow_tcp_ping.unwrap_or(false),
        TaskEventType::HttpPing(_) => server.allow_http_ping.unwrap_or(false),
        TaskEventType::HttpRequest(_) => server.allow_http_request.unwrap_or(false),
        TaskEventType::WebShell(_) => server.allow_web_shell.unwrap_or(false),
        TaskEventType::Execute(_) => server.allow_execute.unwrap_or(false),
        TaskEventType::ReadConfig => server.allow_read_config.unwrap_or(false),
        TaskEventType::EditConfig(_) => server.allow_edit_config.unwrap_or(false),
        TaskEventType::Ip => server.allow_ip.unwrap_or(false),
        TaskEventType::Version => server.allow_version.unwrap_or(false),
        TaskEventType::SelfUpdate(_) => server.allow_self_update.unwrap_or(false),
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
            .and_then(|d| {
                task_type.result_from_duration(d).ok_or_else(|| {
                    NodegetError::Other("Invalid task type for ping duration".to_owned())
                })
            })
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::TcpPing(target) => ping::tcp::tcping_target(target.clone())
            .await
            .and_then(|d| {
                task_type.result_from_duration(d).ok_or_else(|| {
                    NodegetError::Other("Invalid task type for tcp ping duration".to_owned())
                })
            })
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::HttpPing(target) => ping::http::httping_target(target.clone())
            .await
            .and_then(|d| {
                task_type.result_from_duration(d).ok_or_else(|| {
                    NodegetError::Other("Invalid task type for http ping duration".to_owned())
                })
            })
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::HttpRequest(request) => http_request::execute_http_request(request.clone())
            .await
            .map(TaskEventResult::HttpRequest),

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

        TaskEventType::ReadConfig => {
            let args = AGENT_ARGS
                .get()
                .ok_or_else(|| NodegetError::Other("Agent args not initialized".to_owned()))?;
            let file = fs::read_to_string(&args.config)
                .await
                .map_err(|e| NodegetError::Other(format!("Failed to read config file: {e}")))?;
            Ok(TaskEventResult::ReadConfig(file))
        }

        TaskEventType::EditConfig(config_string) => {
            let _parsed: AgentConfig = match toml::from_str(config_string) {
                Ok(config) => config,
                Err(e) => {
                    return Err(NodegetError::Other(format!("Config parse error: {e}")).into());
                }
            };

            let args = AGENT_ARGS
                .get()
                .ok_or_else(|| NodegetError::Other("Agent args not initialized".to_owned()))?;
            fs::write(&args.config, config_string)
                .await
                .map_err(|e| NodegetError::Other(format!("Failed to write config file: {e}")))?;

            Ok(TaskEventResult::EditConfig(true))
        }

        TaskEventType::Ip => {
            let ip_info = ip::ip().await;
            Ok(TaskEventResult::Ip(ip_info.ipv4, ip_info.ipv6))
        }

        TaskEventType::Version => {
            let version = nodeget_lib::utils::version::NodeGetVersion::get();
            Ok(TaskEventResult::Version(version))
        }

        TaskEventType::SelfUpdate(tag) => {
            let success = self_update::self_update(tag).await;
            Ok(TaskEventResult::SelfUpdate(success))
        }
    }
}

// 处理来自服务器的任务请求
//
// 该函数订阅各个服务器的任务通道，接收并执行不同类型的任务（如 Ping、TCP Ping、HTTP Ping、WebShell、命令执行、IP 查询），
// 然后将执行结果返回给服务器。
//
// 与 [`crate::rpc::handle_error_message`] 相同，所有 per-server 订阅循环
// 以及逐任务处理都交给嵌套的 [`JoinSet`] 托管。当主循环在配置热重载
// 时 abort 本函数的顶层 JoinHandle，`JoinSet` 被 drop 并级联 abort
// 全部子任务，避免旧订阅与新订阅同时消费服务器消息。
pub async fn handle_task() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = match get_agent_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to get agent config: {e}");
            return;
        }
    };

    let mut server_tasks = JoinSet::new();

    for server in agent_config.server.unwrap_or_default() {
        server_tasks.spawn(async move {
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

            let mut per_task = JoinSet::new();
            // 这里是 per-server 的"per-message JoinSet"：与外层 `server_tasks`
            // 构成两级 JoinSet 而非两级 tokio::spawn（review_agent.md #69 把这个误读成
            // "嵌套 spawn"）。两级 JoinSet 的好处是：
            //   1. 外层 server 任务被 abort（例如配置热重载）时，JoinSet 本身随作用域 drop，
            //      从而级联 abort 所有在飞的 per-message 任务，不需要额外的
            //      CancellationToken。
            //   2. server 间互相隔离：一个 server 的 broadcast channel 关闭只会结束
            //      自己的 per_task JoinSet，不影响别的 server。
            // task_token / server_name 在外层 clone 一次、在 spawn 内 move，是 async move
            // 闭包必须的开销；再减少也需要换用 Arc，但 token 通常只有几十字节，抗不过
            // 切换至 Arc 带来的 cache-line 争用，权衡后维持 clone。

            loop {
                while let Some(join_result) = per_task.try_join_next() {
                    if let Err(e) = join_result {
                        if !e.is_cancelled() {
                            warn!("[{}] Per-message task failed: {e}", server.name);
                        }
                    }
                }

                let message = match rx.recv().await {
                    Ok(msg) => msg,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[{}] Handle task lagged, dropped {n} messages", server.name);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("[{}] Handle task channel closed", server.name);
                        break;
                    }
                };
                let server_name = server.name.clone();
                let server_token = server.token.clone();
                let server_config = server.clone();
                per_task.spawn(async move {
                    let rpc = match message {
                        Message::Text(text) => text.to_string(),
                        _ => return,
                    };

                    let json_rpc: JsonRpcTask = match serde_json::from_str(&rpc) {
                        Ok(json_rpc) => json_rpc,
                        Err(_) => return,
                    };

                    // 协议上 server 把推送给 agent 的任务 RPC 和 agent 订阅的 RPC
                    // 共用同一个 method 字符串 "task_register_task"：
                    //   - 订阅阶段：agent → server， params 里带 {token}；
                    //   - 任务下发：server → agent， params 里带 TaskEvent。
                    // 这里处理的是"服务器下发的任务"，因此过滤 method；不等于则 silently
                    // 丢弃（多数情况是订阅 ack 或其它共享通道里的 RPC）。
                    // TODO: 协议修订时应改成 task_subscribe /
                    // task_dispatch 两个独立 method，完全避免此类共享含义。
                    if json_rpc.method != "task_register_task" {
                        return;
                    }

                    let task_type = &json_rpc.params.result.task_event_type;

                    let task_result: Result<TaskEventResult> =
                        if is_task_allowed(&server_config, task_type) {
                            // WebShell 是长驻 PTY 会话，天然可长时间运行，
                            // 绕过统一超时；其余任务一律包一层硬上限，
                            // 防止某个被劫持 / 卡死的任务让对应 server
                            // 的 per_task 处理流程永久占用一个 future。
                            let fut = execute_task(
                                task_type,
                                json_rpc.params.result.task_id,
                                &json_rpc.params.result.task_token,
                            );
                            if matches!(task_type, TaskEventType::WebShell(_)) {
                                fut.await
                            } else {
                                match time::timeout(TASK_MAX_TIMEOUT, fut).await {
                                    Ok(res) => res,
                                    Err(_) => Err(NodegetError::Other(format!(
                                        "Task timed out after {}s",
                                        TASK_MAX_TIMEOUT.as_secs()
                                    ))
                                    .into()),
                                }
                            }
                        } else {
                            Err(NodegetError::PermissionDenied(
                                "Permission Denied: Task not allowed".to_owned(),
                            )
                            .into())
                        };

                    let should_restart = matches!(task_type, TaskEventType::EditConfig(_))
                        && matches!(&task_result, Ok(TaskEventResult::EditConfig(true)));

                    let should_self_update_restart =
                        matches!(task_type, TaskEventType::SelfUpdate(_))
                            && matches!(&task_result, Ok(TaskEventResult::SelfUpdate(true)));

                    let timestamp = get_local_timestamp_ms().unwrap_or(0);

                    let agent_uuid = match get_agent_config() {
                        Ok(cfg) => cfg.agent_uuid,
                        Err(e) => {
                            error!("Failed to get agent config for response: {e}");
                            return;
                        }
                    };

                    let response = match task_result {
                        Ok(task_result) => TaskEventResponse {
                            task_id: json_rpc.params.result.task_id,
                            agent_uuid,
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
                                agent_uuid,
                                task_token: json_rpc.params.result.task_token,
                                timestamp,
                                success: false,
                                error_message: Some(error_message),
                                task_event_result: None,
                            }
                        }
                    };

                    let server_token_value = match serde_json::to_value(&server_token) {
                        Ok(v) => v,
                        Err(e) => {
                            // server_token 就是一个 String，理论上这里不可能失败；
                            // 但即便真失败了，我们也宁愿把 token 直接以字符串塞进 JSON
                            // 载荷（server 侧仍能从 error_message 里定位问题），也好过
                            // 直接 return 让任务在 server 一侧永远悬挂。
                            error!("Failed to serialize server token: {e}");
                            serde_json::Value::String(server_token.clone())
                        }
                    };
                    let response_value = match serde_json::to_value(&response) {
                        Ok(v) => v,
                        Err(e) => {
                            // 原始 `response` 只含 primitive 字段（u32/String/bool/Option<…>），
                            // `to_value` 几乎不可能失败；真失败了仍需回 ack 防止 server 把任务
                            // 标为永远 pending (review_agent.md #62)。这里退化成最小 error ack：
                            // 只带 task_id 与原始错误，丢掉 task_event_result 复杂结构。
                            error!("Failed to serialize response: {e}");
                            let fallback = TaskEventResponse {
                                task_id: response.task_id,
                                agent_uuid: response.agent_uuid,
                                task_token: response.task_token.clone(),
                                timestamp: response.timestamp,
                                success: false,
                                error_message: Some(format!(
                                    "agent failed to serialize task response: {e}"
                                )),
                                task_event_result: None,
                            };
                            serde_json::to_value(&fallback).unwrap_or_else(|inner| {
                                // 兜底再兜底：手写一个最小 JSON 对象。
                                error!("Fallback response also failed to serialize: {inner}");
                                serde_json::json!({
                                    "task_id": response.task_id,
                                    "agent_uuid": response.agent_uuid,
                                    "task_token": response.task_token,
                                    "timestamp": response.timestamp,
                                    "success": false,
                                    "error_message": format!(
                                        "agent failed to serialize task response: {e}"
                                    ),
                                    "task_event_result": serde_json::Value::Null,
                                })
                            })
                        }
                    };
                    let rpc = wrap_json_into_rpc_with_id_1(
                        "task_upload_task_result",
                        vec![server_token_value, response_value],
                    );

                    if let Err(e) = send_to(&server_name, Message::Text(Utf8Bytes::from(rpc))).await
                    {
                        error!("{e}");
                    }

                    if should_restart {
                        info!(
                            "[{server_name}] EditConfig applied successfully, restarting agent..."
                        );
                        time::sleep(Duration::from_millis(300)).await;
                        RELOAD_NOTIFY.notify_one();
                    }

                    if should_self_update_restart {
                        info!("[{server_name}] Self-update successful, restarting agent...");
                        time::sleep(Duration::from_millis(300)).await;
                        #[cfg(target_os = "windows")]
                        {
                            nodeget_lib::self_update::restart_process();
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            nodeget_lib::self_update::restart_process_with_exec_v();
                        }
                    }
                });
            }

            // drop per_task -> aborts every in-flight per-message handler
            drop(per_task);
        });
    }

    // Await on the outer JoinSet so the runtime keeps the set alive for the
    // lifetime of this function. A cancel (reload) drops both JoinSets.
    while server_tasks.join_next().await.is_some() {}
}
