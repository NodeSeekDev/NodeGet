//! `nodeget-server` RPC 命名空间——服务器专属操作
//!
//! 提供不属于任何 ng-* crate 的服务器级 RPC 方法：
//! - `hello`、`version`、`uuid`：服务器身份信息
//! - `read_config`、`edit_config`：委托至 `ng_config::server_rpc`
//! - `database_storage`、`exec_sql`、`get_database_type`：委托至 `ng_db::rpc::nodeget`
//! - `log`：从内存日志缓冲区读取历史日志
//! - `stream_log`：实时日志订阅（基于 tracing Layer 广播）
//! - `self_update`：二进制自更新与重启
//!
//! 本模块还负责组装所有 ng-* crate 的 RPC 命名空间，
//! 通过 [`get_modules`] 合并为统一的 `RpcModule`。

use crate::logging;
use jsonrpsee::core::{RpcResult, SubscriptionResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use ng_config::get_server_config;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::version::NodeGetVersion;
use ng_db::rpc::RpcHelper;
use ng_db::rpc::token_identity;
use ng_db::rpc_exec;
use ng_token::check_super_token;
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

/// `nodeget-server` RPC trait 定义
///
/// 使用 `#[rpc]` 宏自动生成客户端/服务端骨架，
/// 命名空间分隔符为 `_`（自定义 jsonrpsee fork）。
#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    /// 返回服务器运行状态确认字符串
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    /// 返回服务器版本信息（语义化版本 + 构建信息）
    #[method(name = "version")]
    async fn version(&self) -> Value;

    /// 返回服务器 UUID（从全局 Config 读取）
    #[method(name = "uuid")]
    async fn uuid(&self) -> String;

    /// 读取服务器配置文件内容
    ///
    /// - token：身份认证凭据
    #[method(name = "read_config")]
    async fn read_config(&self, token: String) -> RpcResult<String>;

    /// 编辑服务器配置文件并触发热重载
    ///
    /// - token：身份认证凭据
    /// - `config_string`：完整的 TOML 配置文本
    #[method(name = "edit_config")]
    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool>;

    /// 查询数据库存储占用（表大小、行数等）
    ///
    /// - token：身份认证凭据
    #[method(name = "database_storage")]
    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>>;

    /// 查询内存日志缓冲区中的历史日志
    ///
    /// - token：身份认证凭据（需 Super Token）
    #[method(name = "log")]
    async fn log(&self, token: String) -> RpcResult<Box<RawValue>>;

    /// 实时日志流订阅
    ///
    /// - token：身份认证凭据（需 Super Token）
    /// - `log_filter`：`EnvFilter` 格式的过滤器字符串，如 `"info,server=debug"`
    #[subscription(name = "stream_log", item = Value, unsubscribe = "unsubscribe_stream_log")]
    async fn stream_log(&self, token: String, log_filter: String) -> SubscriptionResult;

    /// 服务器二进制自更新
    ///
    /// - token：身份认证凭据（需 Super Token）
    /// - tag：目标版本标签，如 `"v0.5.0"`
    #[method(name = "self_update")]
    async fn self_update(&self, token: String, tag: String) -> RpcResult<()>;

    /// 执行原始 SQL 语句
    ///
    /// - token：身份认证凭据
    /// - sql：SQL 语句文本
    /// - params：可选的参数绑定（JSON 对象）
    #[method(name = "exec_sql")]
    async fn exec_sql(
        &self,
        token: String,
        sql: String,
        params: Option<Value>,
    ) -> RpcResult<Box<RawValue>>;

    /// 查询当前数据库类型（PostgreSQL 或 `SQLite`）
    ///
    /// - token：身份认证凭据
    #[method(name = "get_database_type")]
    async fn get_database_type(&self, token: String) -> RpcResult<Box<RawValue>>;
}

/// `nodeget-server` RPC 实现
#[derive(Clone)]
pub struct NodegetServerRpcImpl;

impl RpcHelper for NodegetServerRpcImpl {}

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    /// 返回服务器运行状态确认字符串
    async fn hello(&self) -> String {
        let span = tracing::info_span!(target: "server", "nodeget-server::hello");
        async {
            let response = "NodeGet Server Is Running!".to_string();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    /// 返回服务器版本信息
    async fn version(&self) -> Value {
        let span = tracing::info_span!(target: "server", "nodeget-server::version");
        async {
            let response = serde_json::to_value(NodeGetVersion::get()).unwrap();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    /// 返回服务器 UUID，Config 未初始化时返回空字符串
    async fn uuid(&self) -> String {
        let span = tracing::info_span!(target: "server", "nodeget-server::uuid");
        async {
            let response = get_server_config()
                .and_then(|cfg| cfg.read().ok().map(|c| c.server_uuid.to_string()))
                .unwrap_or_default();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    /// 读取配置文件，委托至 `ng_config::server_rpc::read_config`
    async fn read_config(&self, token: String) -> RpcResult<String> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::read_config", token_key = tk, username = un);
        async {
            match ng_config::server_rpc::read_config(token).await {
                Ok(s) => {
                    tracing::debug!(target: "server", response_len = s.len(), "request completed");
                    Ok(s)
                }
                Err(e) => {
                    tracing::error!(target: "server", error = %e, "request failed");
                    Err(e)
                }
            }
        }
        .instrument(span)
        .await
    }

    /// 编辑配置文件，委托至 `ng_config::server_rpc::edit_config`
    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::edit_config", token_key = tk, username = un, config_len = config_string.len());
        async {
            match ng_config::server_rpc::edit_config(token, config_string).await {
                Ok(b) => {
                    tracing::debug!(target: "server", response = b, "request completed");
                    Ok(b)
                }
                Err(e) => {
                    tracing::error!(target: "server", error = %e, "request failed");
                    Err(e)
                }
            }
        }
        .instrument(span)
        .await
    }

    /// 查询数据库存储占用，委托至 `ng_db::rpc::nodeget::database_storage`
    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::database_storage", token_key = tk, username = un);
        async { rpc_exec!(ng_db::rpc::nodeget::database_storage::database_storage(token).await) }
            .instrument(span)
            .await
    }

    /// 查询内存日志缓冲区
    ///
    /// 内部步骤：
    /// 1. 解析 Token 并验证 Super Token 权限
    /// 2. 从 [`logging::get_memory_logs`] 获取日志快照
    /// 3. 序列化为 JSON `RawValue` 返回
    async fn log(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::log", token_key = tk, username = un);
        async {
            let process_logic = async {
                let token_or_auth = TokenOrAuth::from_full_token(&token).map_err(|e| {
                    ng_core::error::NodegetError::ParseError(format!("Failed to parse token: {e}"))
                })?;

                let is_super = check_super_token(&token_or_auth)
                    .await
                    .map_err(|e| ng_core::error::NodegetError::PermissionDenied(format!("{e}")))?;

                if !is_super {
                    return Err(ng_core::error::NodegetError::PermissionDenied(
                        "Permission Denied: Super token required".to_owned(),
                    )
                    .into());
                }
                tracing::debug!(target: "server", "Super token verified for log query");

                let logs = logging::get_memory_logs();
                tracing::debug!(target: "server", log_count = logs.len(), "In-memory logs fetched");

                serde_json::value::to_raw_value(&logs).map_err(|e| {
                    ng_core::error::NodegetError::SerializationError(e.to_string()).into()
                })
            };
            match process_logic.await {
                Ok(result) => Ok(result),
                Err(e) => Err(ng_db::rpc::to_rpc_error(&e)),
            }
        }
        .instrument(span)
        .await
    }

    /// 实时日志流订阅
    ///
    /// 内部步骤：
    /// 1. 解析 Token 并验证 Super Token 权限，失败则 reject 订阅
    /// 2. 接受订阅，创建 mpsc 通道并注册到 [`logging::StreamLogManager`]
    /// 3. 启动异步转发任务：从 mpsc 接收日志条目 -> 通过 `SubscriptionSink` 推送
    /// 4. 客户端断开后自动注销订阅
    ///
    /// 注意：`add_subscriber` / `remove_subscriber` 持有写锁，内部禁止调用 tracing
    ///       以避免与 `on_event` 的读锁产生死锁（`std::sync::RwLock` 不可重入）
    async fn stream_log(
        &self,
        subscription_sink: PendingSubscriptionSink,
        token: String,
        log_filter: String,
    ) -> SubscriptionResult {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::stream_log", token_key = tk, username = un);
        let _guard = span.enter();

        // ── 身份验证 ──────────────────────────────────────────
        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(target: "server", error = %e, "token parse error, rejecting stream_log subscription");
                subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        101,
                        format!("Token Parse Error: {e}"),
                        None::<()>,
                    ))
                    .await;
                return Ok(());
            }
        };

        let is_super = match check_super_token(&token_or_auth).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(target: "server", error = %e, "super token check failed, rejecting stream_log subscription");
                subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        102,
                        format!("Permission check failed: {e}"),
                        None::<()>,
                    ))
                    .await;
                return Ok(());
            }
        };

        if !is_super {
            tracing::warn!(target: "server", "permission denied, rejecting stream_log subscription");
            subscription_sink
                .reject(jsonrpsee::types::ErrorObject::borrowed(
                    102,
                    "Permission Denied: Super token required",
                    None,
                ))
                .await;
            return Ok(());
        }

        // ── 接受订阅 ─────────────────────────────────────────────
        let sink = subscription_sink.accept().await?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(512);
        let sub_id = Uuid::new_v4();

        let manager = logging::get_stream_log_manager();
        // 注意：此处持有写锁，禁止调用 tracing（见上方文档注释）
        manager.add_subscriber(sub_id, tx, &log_filter);

        // 释放写锁后再输出日志
        tracing::info!(target: "server", sub_id = %sub_id, filter = %log_filter, "stream_log subscription accepted");

        // 释放 span guard 后再 spawn 转发任务，避免 span 跨线程生命周期问题
        drop(_guard);
        let forward_span = span.clone();
        let manager = manager.clone();

        tokio::spawn(async move {
            while let Some(json_str) = rx.recv().await {
                let Ok(raw) = RawValue::from_string(json_str) else {
                    continue;
                };
                let msg = SubscriptionMessage::from(raw);
                if sink.send(msg).await.is_err() {
                    break;
                }
            }

            // 注意：remove_subscriber 持有写锁，内部禁止调用 tracing
            manager.remove_subscriber(&sub_id);
            // 释放写锁后再输出日志
            tracing::info!(target: "server", sub_id = %sub_id, "stream_log subscriber disconnected, removed");
        }.instrument(forward_span));

        Ok(())
    }

    /// 服务器二进制自更新
    ///
    /// 内部步骤：
    /// 1. 解析 Token 并验证 Super Token 权限
    /// 2. 比较当前版本与目标标签，判断是否需要更新
    /// 3. 获取下载 URL
    /// 4. 下载新二进制（超时 2 分钟，最小 1KB 校验）
    /// 5. 替换当前二进制文件
    /// 6. 在 Unix 上设置可执行权限 (0o755)
    /// 7. 延迟 3 秒后重启进程（等待 RPC 响应返回客户端）
    async fn self_update(&self, token: String, tag: String) -> RpcResult<()> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::self_update", token_key = tk, username = un, tag = %tag);
        async {
            let process_logic: anyhow::Result<()> = async {
                let token_or_auth = TokenOrAuth::from_full_token(&token)
                    .map_err(|e| ng_core::error::NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

                let is_super = check_super_token(&token_or_auth)
                    .await
                    .map_err(|e| ng_core::error::NodegetError::PermissionDenied(format!("{e}")))?;

                if !is_super {
                    return Err(ng_core::error::NodegetError::PermissionDenied(
                        "Permission Denied: Super token required".to_owned(),
                    )
                        .into());
                }
                tracing::debug!(target: "server", "Super token verified for self_update");

                // 1. 检查是否需要更新
                let (current_version, target_version, should_update) =
                    ng_core::self_update::check_if_update_needed(&tag);

                if !should_update {
                    tracing::info!(
                        target: "server",
                        current = %format!("{}.{}.{}", current_version.0, current_version.1, current_version.2),
                        target = %format!("{}.{}.{}", target_version.0, target_version.1, target_version.2),
                        "Server is up to date"
                    );
                    return Ok(());
                }

                tracing::info!(
                    target: "server",
                    current = %format!("{}.{}.{}", current_version.0, current_version.1, current_version.2),
                    target = %format!("{}.{}.{}", target_version.0, target_version.1, target_version.2),
                    "Server update available, downloading..."
                );

                // 3. 获取下载 URL
                let url = ng_core::self_update::get_server_url(&tag).ok_or_else(|| {
                    ng_core::error::NodegetError::Other(format!("Failed to get download URL for tag: {tag}"))
                })?;

                tracing::info!(target: "server", url = %url, "Downloading update");

                // 4. 下载二进制
                let client = reqwest::Client::new();
                let response = client
                    .get(&url)
                    .header("User-Agent", "NodeGet-Server")
                    .timeout(std::time::Duration::from_mins(2))
                    .send()
                    .await
                    .map_err(|e| ng_core::error::NodegetError::Other(format!("Download request failed: {e}")))?;

                if !response.status().is_success() {
                    return Err(ng_core::error::NodegetError::Other(format!(
                        "Download failed with status: {}",
                        response.status()
                    ))
                        .into());
                }

                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| ng_core::error::NodegetError::Other(format!("Failed to read response body: {e}")))?;

                if bytes.len() < 1024 {
                    return Err(ng_core::error::NodegetError::Other(format!(
                        "Downloaded file too small ({} bytes), aborting",
                        bytes.len()
                    ))
                        .into());
                }

                tracing::info!(target: "server", size = bytes.len(), "Update downloaded");

                // 5. 替换二进制
                if !ng_core::self_update::replace_binary(bytes.to_vec()) {
                    return Err(ng_core::error::NodegetError::Other("Failed to replace binary".to_owned()).into());
                }

                // 6. 在 Unix 上设置可执行权限
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let current = ng_core::self_update::canonical_exe_path().ok_or_else(|| {
                        ng_core::error::NodegetError::Other("Failed to get canonical exe path".to_owned())
                    })?;
                    let perms = std::fs::Permissions::from_mode(0o755);
                    if let Err(e) = std::fs::set_permissions(&current, perms) {
                        tracing::warn!(target: "server", error = %e, "Failed to set executable permission");
                    }
                }

                tracing::info!(target: "server", "Binary replaced successfully, scheduling restart");

                // 7. 延迟重启，等待 RPC 响应返回客户端
                tokio::spawn(async {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    tracing::info!(target: "server", "Restarting server...");
                    #[cfg(target_os = "windows")]
                    {
                        ng_core::self_update::restart_process();
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        ng_core::self_update::restart_process_with_exec_v();
                    }
                });

                Ok(())
            }
                .await;

            match process_logic {
                Ok(()) => Ok(()),
                Err(e) => Err(ng_db::rpc::to_rpc_error(&e)),
            }
        }
            .instrument(span)
            .await
    }

    /// 执行原始 SQL，委托至 `ng_db::rpc::nodeget::exec_sql`
    async fn exec_sql(
        &self,
        token: String,
        sql: String,
        params: Option<Value>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::exec_sql", token_key = tk, username = un, sql_len = sql.len());
        async { rpc_exec!(ng_db::rpc::nodeget::exec_sql::exec_sql(token, sql, params).await) }
            .instrument(span)
            .await
    }

    /// 查询数据库类型，委托至 `ng_db::rpc::nodeget::get_database_type`
    async fn get_database_type(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::get_database_type", token_key = tk, username = un);
        async { rpc_exec!(ng_db::rpc::nodeget::get_database_type::get_database_type(token).await) }
            .instrument(span)
            .await
    }
}

/// 构建并缓存合并后的 RPC 模块
///
/// 使用 `OnceLock` 缓存：首次调用时构建，后续调用返回克隆实例。
/// 避免重复 merge 开销。
///
/// 内部步骤：
/// 1. 创建空 `RpcModule`
/// 2. 依次 merge 各 ng-* crate 的命名空间（见下方注释）
/// 3. 返回合并后的模块
pub fn get_modules() -> jsonrpsee::RpcModule<()> {
    static GLOBAL_RPC_MODULE: std::sync::OnceLock<jsonrpsee::RpcModule<()>> =
        std::sync::OnceLock::new();
    GLOBAL_RPC_MODULE.get_or_init(build_modules).clone()
}

/// 组装所有 RPC 命名空间
fn build_modules() -> jsonrpsee::RpcModule<()> {
    let mut module = jsonrpsee::RpcModule::new(());

    // nodeget-server 命名空间（服务器专属）
    module
        .merge(NodegetServerRpcImpl.into_rpc())
        .expect("failed to merge nodeget-server RPC");

    // ng-monitoring：agent + agent-uuid + nodeget-server.list_all_agent_uuid
    module
        .merge(ng_monitoring::rpc_module())
        .expect("failed to merge ng-monitoring RPC");

    // ng-task：task 命名空间
    module
        .merge(ng_task::rpc_module())
        .expect("failed to merge ng-task RPC");

    // ng-token：token 命名空间
    module
        .merge(ng_token::rpc_module())
        .expect("failed to merge ng-token RPC");

    // ng-kv：kv 命名空间
    module
        .merge(ng_kv::rpc_module())
        .expect("failed to merge ng-kv RPC");

    // ng-static：static-bucket + static-bucket-file 命名空间
    module
        .merge(ng_static::rpc::rpc_module())
        .expect("failed to merge ng-static RPC");

    // ng-db：db 命名空间
    module
        .merge(ng_db::rpc::db::rpc_module())
        .expect("failed to merge ng-db RPC");

    // ng-js-worker：js-worker + js-result 命名空间
    module
        .merge(ng_js_worker::rpc_module())
        .expect("failed to merge ng-js-worker RPC");

    // ng-crontab：crontab + crontab_result 命名空间
    module
        .merge(ng_crontab::rpc_module())
        .expect("failed to merge ng-crontab RPC");

    module
}
