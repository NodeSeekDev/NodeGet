use crate::get_db;
use ng_core::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, to_value};
use std::fmt;
use std::sync::{Arc, OnceLock};

#[cfg(feature = "server")]
pub mod db;
#[cfg(feature = "server")]
pub mod nodeget;

// ── Auth provider trait ────────────────────────────────────────────

/// Trait for authentication and authorization operations.
/// The server crate implements this to provide concrete auth checking.
#[cfg(feature = "server")]
pub trait AuthProvider: Send + Sync + 'static {
    fn check_token_limit(
        &self,
        token_or_auth: &ng_core::permission::token_auth::TokenOrAuth,
        scopes: Vec<ng_core::permission::data_structure::Scope>,
        permissions: Vec<ng_core::permission::data_structure::Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send>>;

    fn check_super_token(
        &self,
        token_or_auth: &ng_core::permission::token_auth::TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send>>;
}

#[cfg(feature = "server")]
static AUTH_PROVIDER: OnceLock<Arc<dyn AuthProvider>> = OnceLock::new();

#[cfg(feature = "server")]
pub fn set_auth_provider(provider: Arc<dyn AuthProvider>) {
    let _ = AUTH_PROVIDER.set(provider);
}

#[cfg(feature = "server")]
pub fn auth_provider() -> Option<&'static Arc<dyn AuthProvider>> {
    AUTH_PROVIDER.get()
}

// ── RPC tracing utilities ───────────────────────────────────────────

/// Lightweight extraction of `(token_key, username)` from a raw token string.
///
/// - Token mode (`key:secret`): returns `(key, "")`
/// - Auth mode (`username|password`): returns `("", username)`
/// - Fallback: returns `("???", "")`
///
/// Zero-allocation: returns borrowed slices into the original string.
#[must_use]
pub fn token_identity(token: &str) -> (&str, &str) {
    token.find(':').map_or_else(
        || {
            token
                .find('|')
                .map_or(("???", ""), |pipe| ("", &token[..pipe]))
        },
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
#[macro_export]
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

pub trait RpcHelper {
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val).map(Set).map_err(|e| {
            NodegetError::SerializationError(format!("Serialization error: {e}")).into()
        })
    }

    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}

/// Helper to convert an anyhow error into a JSON-RPC error response.
#[must_use]
pub fn to_rpc_error(e: &anyhow::Error) -> jsonrpsee::types::ErrorObject<'static> {
    let nodeget_err = ng_core::error::anyhow_to_nodeget_error(e);
    jsonrpsee::types::ErrorObject::owned(
        nodeget_err.error_code() as i32,
        format!("{nodeget_err}"),
        None::<()>,
    )
}
