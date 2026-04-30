use crate::DB;
use crate::rpc::nodeget::NodegetServerRpcImpl;
use jsonrpsee::RpcModule;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, to_value};
use std::fmt;
use std::sync::OnceLock;

pub mod agent;
pub mod crontab;
pub mod crontab_result;
pub mod js_result;
pub mod js_worker;
pub mod kv;
pub mod nodeget;
pub mod task;
pub mod token;

// ── RPC tracing utilities ───────────────────────────────────────────

/// Lightweight extraction of `(token_key, username)` from a raw token string.
///
/// - Token mode (`key:secret`): returns `(key, "")`
/// - Auth mode (`username|password`): returns `("", username)`
/// - Fallback: returns `("???", "")`
///
/// Zero-allocation: returns borrowed slices into the original string.
pub fn token_identity(token: &str) -> (&str, &str) {
    token.find(':').map_or_else(
        || token.find('|').map_or(("???", ""), |pipe| ("", &token[..pipe])),
        |colon| (&token[..colon], ""),
    )
}

/// A wrapper around `&RawValue` that truncates its `Display` output to 1024 bytes.
pub struct TruncatedRaw<'a>(pub &'a RawValue);

impl fmt::Display for TruncatedRaw<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MAX: usize = 1024;
        let s = self.0.get();
        if s.len() <= MAX {
            f.write_str(s)
        } else {
            let end = s.floor_char_boundary(MAX);
            f.write_str(&s[..end])?;
            write!(f, "[...{} bytes total]", s.len())
        }
    }
}

/// Common log pattern for RPC methods returning `RpcResult<Box<RawValue>>`.
///
/// Usage: `rpc_exec!(some_inner_call(args).await)`
///
/// Emits:
/// - `debug response=<truncated> "request completed"` on success
/// - `error error=<e> "request failed"` on failure
///
/// Note: the timing middleware already logs per-request timing at the
/// configured level, so the macro only logs the outcome.
///
/// Uses `target: "rpc"` intentionally — this is cross-cutting RPC
/// infrastructure logging, distinct from domain-specific targets
/// (kv, token, `js_worker`, etc.).
macro_rules! rpc_exec {
    ($expr:expr) => {{
        match $expr {
            Ok(raw) => {
                tracing::debug!(target: "rpc", response = %$crate::rpc::TruncatedRaw(&raw), "request completed");
                Ok(raw)
            }
            Err(e) => {
                tracing::error!(target: "rpc", error = %e, "request failed");
                Err(e)
            }
        }
    }};
}

pub(crate) use rpc_exec;

pub trait RpcHelper {
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val).map(Set).map_err(|e| {
            NodegetError::SerializationError(format!("Serialization error: {e}")).into()
        })
    }

    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        DB.get()
            .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}

static GLOBAL_RPC_MODULE: OnceLock<RpcModule<NodegetServerRpcImpl>> = OnceLock::new();

pub fn get_modules() -> RpcModule<NodegetServerRpcImpl> {
    GLOBAL_RPC_MODULE.get_or_init(build_modules).clone()
}

fn build_modules() -> RpcModule<NodegetServerRpcImpl> {
    use crate::rpc::agent::RpcServer as AgentRpcServer;
    use crate::rpc::crontab::RpcServer as CrontabRpcServer;
    use crate::rpc::crontab_result::RpcServer as CrontabResultRpcServer;
    use crate::rpc::js_result::RpcServer as JsResultRpcServer;
    use crate::rpc::js_worker::RpcServer as JsWorkerRpcServer;
    use crate::rpc::kv::RpcServer as KvRpcServer;
    use crate::rpc::nodeget::RpcServer as NodeGetRpcServer;
    use crate::rpc::task::RpcServer as TaskRpcServer;
    use crate::rpc::token::RpcServer as TokenRpcServer;

    let task_manager = task::TaskManager::global().clone();

    let mut rpc_module = NodegetServerRpcImpl.into_rpc();

    rpc_module.merge(agent::AgentRpcImpl.into_rpc()).unwrap();

    rpc_module
        .merge(
            task::TaskRpcImpl {
                manager: task_manager,
            }
            .into_rpc(),
        )
        .unwrap();

    rpc_module.merge(token::TokenRpcImpl.into_rpc()).unwrap();

    rpc_module.merge(kv::KvRpcImpl.into_rpc()).unwrap();

    rpc_module
        .merge(js_worker::JsWorkerRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(crontab::CrontabRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(crontab_result::CrontabResultRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(js_result::JsResultRpcImpl.into_rpc())
        .unwrap();

    rpc_module
}
