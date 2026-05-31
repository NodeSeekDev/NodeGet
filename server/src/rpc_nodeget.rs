//! nodeget-server RPC namespace — server-level operations not in any ng-* crate.
//!
//! This module provides the `nodeget-server` namespace RPC implementation
//! for methods that are inherently server-binary-specific:
//! - `hello`, `version`, `uuid` — server identity
//! - `read_config`, `edit_config` — delegates to `ng_config::server_rpc`
//! - `database_storage` — delegates to `ng_db::rpc::nodeget::database_storage`
//! - `exec_sql` — delegates to `ng_db::rpc::nodeget::exec_sql`
//! - `get_database_type` — delegates to `ng_db::rpc::nodeget::get_database_type`
//! - `log`, `stream_log` — in-memory log buffer and real-time subscription
//! - `self_update` — binary self-update

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

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    #[method(name = "version")]
    async fn version(&self) -> Value;

    #[method(name = "uuid")]
    async fn uuid(&self) -> String;

    #[method(name = "read_config")]
    async fn read_config(&self, token: String) -> RpcResult<String>;

    #[method(name = "edit_config")]
    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool>;

    #[method(name = "database_storage")]
    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "log")]
    async fn log(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[subscription(name = "stream_log", item = Value, unsubscribe = "unsubscribe_stream_log")]
    async fn stream_log(&self, token: String, log_filter: String) -> SubscriptionResult;

    #[method(name = "self_update")]
    async fn self_update(&self, token: String, tag: String) -> RpcResult<()>;

    #[method(name = "exec_sql")]
    async fn exec_sql(
        &self,
        token: String,
        sql: String,
        params: Option<Value>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "get_database_type")]
    async fn get_database_type(&self, token: String) -> RpcResult<Box<RawValue>>;
}

#[derive(Clone)]
pub struct NodegetServerRpcImpl;

impl RpcHelper for NodegetServerRpcImpl {}

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
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

    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::database_storage", token_key = tk, username = un);
        async { rpc_exec!(ng_db::rpc::nodeget::database_storage::database_storage(token).await) }
            .instrument(span)
            .await
    }

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

                let json_str = serde_json::to_string(&logs)
                    .map_err(|e| ng_core::error::NodegetError::SerializationError(e.to_string()))?;

                RawValue::from_string(json_str).map_err(|e| {
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

    async fn stream_log(
        &self,
        subscription_sink: PendingSubscriptionSink,
        token: String,
        log_filter: String,
    ) -> SubscriptionResult {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::stream_log", token_key = tk, username = un);
        let _guard = span.enter();

        // ── Authentication ──────────────────────────────────────────
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

        // ── Accept subscription ─────────────────────────────────────
        let sink = subscription_sink.accept().await?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(512);
        let sub_id = Uuid::new_v4();

        let manager = logging::get_stream_log_manager();
        // NOTE: no tracing calls here – add_subscriber holds the write lock
        manager.add_subscriber(sub_id, tx, &log_filter);

        // Log *after* the lock is released
        tracing::info!(target: "server", sub_id = %sub_id, filter = %log_filter, "stream_log subscription accepted");

        // Drop span guard before spawning the forwarding task
        drop(_guard);
        let forward_span = span.clone();
        let manager = manager.clone();

        tokio::spawn(async move {
            while let Some(entry) = rx.recv().await {
                let Ok(json_str) = serde_json::to_string(&entry) else {
                    continue;
                };
                let Ok(raw) = RawValue::from_string(json_str) else {
                    continue;
                };
                let msg = SubscriptionMessage::from(raw);
                if sink.send(msg).await.is_err() {
                    break;
                }
            }

            // NOTE: no tracing calls inside remove_subscriber (holds write lock)
            manager.remove_subscriber(&sub_id);
            // Log after lock is released
            tracing::info!(target: "server", sub_id = %sub_id, "stream_log subscriber disconnected, removed");
        }.instrument(forward_span));

        Ok(())
    }

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

                // 1. Check if update needed
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

                // 3. Get download URL
                let url = ng_core::self_update::get_server_url(&tag).ok_or_else(|| {
                    ng_core::error::NodegetError::Other(format!("Failed to get download URL for tag: {tag}"))
                })?;

                tracing::info!(target: "server", url = %url, "Downloading update");

                // 4. Download binary
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

                // 5. Replace binary
                if !ng_core::self_update::replace_binary(bytes.to_vec()) {
                    return Err(ng_core::error::NodegetError::Other("Failed to replace binary".to_owned()).into());
                }

                // 6. Set executable permission on Unix
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

                // 7. Spawn delayed restart to allow response to return
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

    async fn get_database_type(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::get_database_type", token_key = tk, username = un);
        async { rpc_exec!(ng_db::rpc::nodeget::get_database_type::get_database_type(token).await) }
            .instrument(span)
            .await
    }
}

/// Build the merged RPC module from all ng-* crates and the server's own nodeget-server namespace.
///
/// Uses `OnceLock` to cache the module — the first call builds it, subsequent calls
/// return a clone of the cached instance.
pub fn get_modules() -> jsonrpsee::RpcModule<()> {
    static GLOBAL_RPC_MODULE: std::sync::OnceLock<jsonrpsee::RpcModule<()>> =
        std::sync::OnceLock::new();
    GLOBAL_RPC_MODULE.get_or_init(build_modules).clone()
}

fn build_modules() -> jsonrpsee::RpcModule<()> {
    let mut module = jsonrpsee::RpcModule::new(());

    // nodeget-server namespace（服务器专属）
    module
        .merge(NodegetServerRpcImpl.into_rpc())
        .expect("failed to merge nodeget-server RPC");

    // ng-monitoring: agent + agent-uuid + nodeget-server.list_all_agent_uuid
    module
        .merge(ng_monitoring::rpc_module())
        .expect("failed to merge ng-monitoring RPC");

    // ng-task: task namespace
    module
        .merge(ng_task::rpc_module())
        .expect("failed to merge ng-task RPC");

    // ng-token: token namespace
    module
        .merge(ng_token::rpc_module())
        .expect("failed to merge ng-token RPC");

    // ng-kv: kv namespace
    module
        .merge(ng_kv::rpc_module())
        .expect("failed to merge ng-kv RPC");

    // ng-static: static-bucket + static-bucket-file namespaces
    module
        .merge(ng_static::rpc::rpc_module())
        .expect("failed to merge ng-static RPC");

    // ng-db: db namespace
    module
        .merge(ng_db::rpc::db::rpc_module())
        .expect("failed to merge ng-db RPC");

    // ng-js-worker: js-worker + js-result namespaces
    module
        .merge(ng_js_worker::rpc_module())
        .expect("failed to merge ng-js-worker RPC");

    // ng-crontab: crontab + crontab_result namespaces
    module
        .merge(ng_crontab::rpc_module())
        .expect("failed to merge ng-crontab RPC");

    module
}
