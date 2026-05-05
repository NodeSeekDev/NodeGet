use crate::rpc::RpcHelper;
use crate::rpc::{rpc_exec, token_identity};
use crate::{RELOAD_NOTIFY, SERVER_CONFIG, SERVER_CONFIG_PATH};
use jsonrpsee::core::{RpcResult, SubscriptionResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use nodeget_lib::config::server::ServerConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

pub mod config;
pub mod database_storage;
pub mod list_all_agent_uuid;
pub mod log_query;
pub mod self_update;

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    #[method(name = "version")]
    async fn version(&self) -> Value;

    #[method(name = "uuid")]
    async fn uuid(&self) -> String;

    #[method(name = "list_all_agent_uuid")]
    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>>;

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
    async fn self_update(&self, token: String) -> RpcResult<()>;
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
            let response = SERVER_CONFIG
                .get()
                .and_then(|cfg| cfg.read().ok().map(|c| c.server_uuid.to_string()))
                .unwrap_or_default();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::list_all_agent_uuid", token_key = tk, username = un);
        async { rpc_exec!(list_all_agent_uuid::list_all_agent_uuid(token).await) }
            .instrument(span)
            .await
    }

    async fn read_config(&self, token: String) -> RpcResult<String> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::read_config", token_key = tk, username = un);
        async {
            match config::read_config(token).await {
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
            match config::edit_config(token, config_string).await {
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
        async { rpc_exec!(database_storage::database_storage(token).await) }
            .instrument(span)
            .await
    }

    async fn log(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::log", token_key = tk, username = un);
        async { rpc_exec!(log_query::query_logs(token).await) }
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

        let is_super = match crate::token::super_token::check_super_token(&token_or_auth).await {
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

        let manager = crate::logging::get_stream_log_manager();
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

    async fn self_update(&self, token: String) -> RpcResult<()> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::self_update", token_key = tk, username = un);
        async {
            match self_update::self_update(token).await {
                Ok(()) => {
                    tracing::debug!(target: "server", "self_update completed");
                    Ok(())
                }
                Err(e) => {
                    tracing::error!(target: "server", error = %e, "self_update failed");
                    Err(e)
                }
            }
        }
        .instrument(span)
        .await
    }
}
