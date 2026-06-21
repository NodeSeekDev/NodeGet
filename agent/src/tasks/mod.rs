//! 任务处理模块。
//!
//! 接收 Server 下发的各类任务（ICMP/TCP/HTTP Ping、命令执行、HTTP 请求、
//! DNS 查询、IP 获取、WebShell PTY、配置读写、自更新等），执行后将结果上报。
//! 任务权限由 Server 配置的 `allow_*` 或 `allow_task_type` 列表控制。
//!
//! 核心循环 [`handle_task`] 订阅各 Server 的下行消息通道，过滤出任务 RPC 并派发执行。
//!
//! ## 网络 I/O 任务池
//!
//! ICMP/TCP/HTTP Ping、HTTP Request、IP 查询、DNS 查询等网络任务共用一个
//! 全局 [`TaskPool`]（信号量 + 硬超时），限制并发不超过 [`TASK_POOL_MAX_CONCURRENCY`]，
//! 单个任务执行不超过 [`TASK_POOL_PER_TASK_TIMEOUT`]，避免多 server 同时下发时
//! 打爆文件描述符或耗尽连接池。

use crate::config_access::get_agent_config;
use crate::rpc::multi_server::{send_to, subscribe_to};
use crate::rpc::{JsonRpcTask, wrap_json_into_rpc_with_id_1};
use crate::{AGENT_ARGS, RELOAD_NOTIFY};
use log::{error, info, warn};
use ng_config::config::agent::AgentConfig;
use ng_core::error::NodegetError;
use ng_core::utils::get_local_timestamp_ms;
use ng_task::{TaskEventResponse, TaskEventResult, TaskEventType};
use std::time::Duration;
use tokio::sync::Semaphore;
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

/// 网络 I/O 任务池最大并发数。
const TASK_POOL_MAX_CONCURRENCY: usize = 10;

/// 网络 I/O 任务池中单个任务的硬性执行超时。
///
/// 覆盖各子任务自有的超时（ICMP 2s、TCP 1s、HTTP Ping 10s、
/// HTTP Request 30s、IP 5s、DNS 视服务器而定），作为兜底上限：
/// 正常任务在各自超时内完成；若子任务超时失效（如 reqwest Client
/// 构建异常），此硬超时确保不会无限占用池中插槽。
const TASK_POOL_PER_TASK_TIMEOUT: Duration = Duration::from_secs(10);

/// 全局网络 I/O 任务池。
///
/// 使用 [`Semaphore`] 限制同时运行的网络任务数量，避免多 server
/// 同时下发大量 ping / `http_request` / dns 任务时打爆 FD 或耗尽连接池。
/// 超过并发上限的任务会在 `acquire()` 处等待；等待期间不占用执行资源。
struct TaskPool {
    semaphore: Semaphore,
}

impl TaskPool {
    /// 创建指定并发上限的任务池。
    fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Semaphore::new(max_concurrency),
        }
    }

    /// 在池中执行一个异步任务。
    ///
    /// 1. 等待获取信号量许可（排队）
    /// 2. 获取许可后，以 [`TASK_POOL_PER_TASK_TIMEOUT`] 硬超时执行 `fut`
    /// 3. 超时则返回错误，许可自动释放
    ///
    /// 计时只覆盖实际执行阶段；排队等待时间不计入超时。
    async fn run<F, T>(&self, fut: F) -> std::result::Result<T, NodegetError>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| NodegetError::Other("Task pool closed".to_owned()))?;
        time::timeout(TASK_POOL_PER_TASK_TIMEOUT, fut)
            .await
            .map_err(|_| {
                NodegetError::Other(format!(
                    "Network task timed out after {}s (pool limit)",
                    TASK_POOL_PER_TASK_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| NodegetError::Other(format!("{e}")))
    }
}

/// 全局任务池单例。
static TASK_POOL: std::sync::LazyLock<TaskPool> =
    std::sync::LazyLock::new(|| TaskPool::new(TASK_POOL_MAX_CONCURRENCY));

/// WebShell（PTY）同时活跃会话数上限。
///
/// WebShell 是长驻 PTY 会话，每个会话占用一个 tokio task + 一个 PTY 子进程 +
/// per_task JoinSet 插槽。无上限时恶意/异常 server 下发大量 WebShell 任务会
/// 在小内存机器上累积子进程与 FD，耗尽资源。用信号量限制活跃会话数，超额
/// 直接失败上报（不排队，避免堆积）。
const WEBSHELL_MAX_SESSIONS: usize = 8;

/// 全局 WebShell 活跃会话信号量。
static WEBSHELL_SESSION_SEMAPHORE: std::sync::LazyLock<Semaphore> =
    std::sync::LazyLock::new(|| Semaphore::new(WEBSHELL_MAX_SESSIONS));

/// 判断任务类型是否应纳入网络 I/O 任务池。
///
/// 仅对涉及网络 I/O 的短任务限流；长驻会话（WebShell）、本地操作
///（ReadConfig/EditConfig/Version）、有独立进程管理的 Execute/SelfUpdate
/// 不纳入池。
const fn is_pool_managed(task_type: &TaskEventType) -> bool {
    matches!(
        task_type,
        TaskEventType::Ping(_)
            | TaskEventType::TcpPing(_)
            | TaskEventType::HttpPing(_)
            | TaskEventType::HttpRequest(_)
            | TaskEventType::Ip
            | TaskEventType::Dns(_)
    )
}

/// DNS 查询模块
mod dns;
/// 命令执行模块
mod execute;
/// HTTP Request 任务模块
mod http_request;
/// IP 获取模块
mod ip;
/// Ping 任务模块
pub mod ping;
/// PTY（伪终端）模块
mod pty;
/// 自更新模块
pub mod self_update;

/// 检查 Server 是否允许执行特定类型的任务。
///
/// 若 Server 配置了 `allow_task_type`（非空），以此列表为准，忽略所有单独的 `allow_*` 开关；
/// 未指定或为空时，回退到原有的布尔开关。
///
/// - `server` - 服务器配置
/// - `task_type` - 任务类型
///
/// 返回是否允许执行。
fn is_task_allowed(server: &ng_config::config::agent::Server, task_type: &TaskEventType) -> bool {
    // 若指定了 allow_task_type（非空），则以此列表为准，忽略所有单独的 allow_* 开关
    if let Some(ref allowed) = server.allow_task_type
        && !allowed.is_empty()
    {
        let task_name = task_type.task_name();
        return allowed.iter().any(|t| t == task_name);
    }

    // 未指定 allow_task_type 或为空时，回退到原有的布尔开关
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
        TaskEventType::Dns(_) => server.allow_dns.unwrap_or(false),
        TaskEventType::Version => server.allow_version.unwrap_or(false),
        TaskEventType::SelfUpdate(_) => server.allow_self_update.unwrap_or(false),
    }
}

/// 执行具体任务，根据任务类型派发到对应的处理函数。
///
/// 网络 I/O 类任务（ICMP/TCP/HTTP Ping、HTTP Request、IP、DNS）通过
/// [`TASK_POOL`] 限流执行，最多 [`TASK_POOL_MAX_CONCURRENCY`] 个并发，
/// 单个硬超时 [`TASK_POOL_PER_TASK_TIMEOUT`]；其余任务直接执行。
///
/// - `task_type` - 任务类型枚举
/// - `task_id` - 任务 ID
/// - `task_token` - 任务令牌
/// - `ignore_cert` - 是否跳过 TLS 证书校验
///
/// 返回任务执行结果；执行失败时返回错误。
async fn execute_task(
    task_type: &TaskEventType,
    task_id: u64,
    task_token: &str,
    ignore_cert: bool,
) -> Result<TaskEventResult> {
    if is_pool_managed(task_type) {
        return execute_task_via_pool(task_type, task_id, task_token, ignore_cert).await;
    }

    execute_task_direct(task_type, task_id, task_token, ignore_cert).await
}

/// 通过全局任务池执行网络 I/O 任务（限流 + 硬超时）。
async fn execute_task_via_pool(
    task_type: &TaskEventType,
    task_id: u64,
    task_token: &str,
    ignore_cert: bool,
) -> Result<TaskEventResult> {
    // 在 move 闭包中需要 owned 的 task_type 副本；caller 传入的是引用，
    // 但到此处 task_type 一定是 pool-managed 类型，clone 开销可忽略
    // （String 内含 Arc，大多数 variant 只有一个 String 或两个）。
    let task_type_owned = task_type.clone();
    let fut = async move {
        execute_task_direct(&task_type_owned, task_id, task_token, ignore_cert).await
    };
    Box::pin(TASK_POOL.run(fut)).await.map_err(Into::into)
}

/// 直接执行任务（不限流），按任务类型派发到对应处理函数。
async fn execute_task_direct(
    task_type: &TaskEventType,
    task_id: u64,
    task_token: &str,
    ignore_cert: bool,
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
            pty::handle_pty_url(url, terminal_id, ignore_cert)
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

        TaskEventType::Dns(dns_task) => dns::query_dns(dns_task)
            .await
            .map(TaskEventResult::Dns)
            .map_err(|e| NodegetError::Other(format!("{e}")).into()),

        TaskEventType::Version => {
            let version = ng_core::utils::version::NodeGetVersion::get().clone();
            Ok(TaskEventResult::Version(version))
        }

        TaskEventType::SelfUpdate(tag) => {
            let success = self_update::self_update(tag).await;
            Ok(TaskEventResult::SelfUpdate(success))
        }
    }
}

/// 处理来自 Server 的任务请求。
///
/// 订阅各 Server 的下行消息通道，接收并执行不同类型的任务（Ping、WebShell、
/// 命令执行、IP 查询、DNS 查询等），然后将执行结果上报。
///
/// 与 [`crate::rpc::handle_error_message`] 相同，所有 per-server 订阅循环
/// 以及逐任务处理都交给嵌套的 [`JoinSet`] 托管。当主循环在配置热重载
/// 时 abort 本函数的顶层 `JoinHandle`，`JoinSet` 被 drop 并级联 abort
/// 全部子任务，避免旧订阅与新订阅同时消费服务器消息。
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

    for server in agent_config.server.as_deref().unwrap_or_default() {
        let server = server.clone();
        server_tasks.spawn(async move {
            if !server.allow_task.unwrap_or(false) {
                return;
            }
            let mut rx: tokio::sync::broadcast::Receiver<std::sync::Arc<serde_json::Value>> =
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
                    if let Err(e) = join_result
                        && !e.is_cancelled()
                    {
                        warn!("[{}] Per-message task failed: {e}", server.name);
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
                    // 下行通道广播的已是解析好的 `Arc<Value>`，这里用
                    // `from_value`（克隆 Value 遍历）而非 `from_str`，省去重复解析。
                    // 非任务消息（解析失败 / method 不匹配）silently 丢弃，与原逻辑一致。
                    let json_rpc: JsonRpcTask = match serde_json::from_value((*message).clone()) {
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
                            let fut = Box::pin(execute_task(
                                task_type,
                                json_rpc.params.result.task_id,
                                &json_rpc.params.result.task_token,
                                server_config.ignore_cert.unwrap_or(false),
                            ));
                            if matches!(task_type, TaskEventType::WebShell(_)) {
                                // WebShell 长驻会话上限：try_acquire 非阻塞，满则立即失败上报，
                                // 防止恶意/异常 server 下发大量 WebShell 在小内存机器上累积
                                // PTY 子进程。permit 持有到 fut 结束自动释放。
                                match WEBSHELL_SESSION_SEMAPHORE.try_acquire() {
                                    Ok(_session_permit) => fut.await,
                                    Err(_) => Err(NodegetError::Other(format!(
                                        "WebShell session limit reached (max {WEBSHELL_MAX_SESSIONS} concurrent sessions)"
                                    )).into()),
                                }
                            } else {
                                time::timeout(TASK_MAX_TIMEOUT, fut)
                                    .await
                                    .unwrap_or_else(|_| {
                                        Err(NodegetError::Other(format!(
                                            "Task timed out after {}s",
                                            TASK_MAX_TIMEOUT.as_secs()
                                        ))
                                        .into())
                                    })
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
                            ng_core::self_update::restart_process();
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            ng_core::self_update::restart_process_with_exec_v();
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
