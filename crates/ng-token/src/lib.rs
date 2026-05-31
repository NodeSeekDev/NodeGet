//! ng-token: Token management for NodeGet.
//!
//! ## Default features (types only)
//! Token, Limit, Scope, Permission, TokenOrAuth are re-exported from ng-core.
//!
//! ## `server` feature
//! - `TokenCache` — DB-backed in-memory token cache
//! - `super_token` — super token generation, rolling, and verification
//! - `generate_token` — child token generation
//! - `get` — token lookup and permission checking
//! - RPC namespace — `token.*` JSON-RPC methods
//! - `AuthChecker` impl — integration with ng-infra's global auth injection

// ── Re-exports from ng-core (always available) ──────────────────────

pub use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
pub use ng_core::permission::data_structure::{Limit, Permission, Scope, Token};
pub use ng_core::permission::token_auth::TokenOrAuth;

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod cache;
#[cfg(feature = "server")]
pub mod generate_token;
#[cfg(feature = "server")]
pub mod get;
#[cfg(feature = "server")]
pub mod rpc;
#[cfg(feature = "server")]
pub mod super_token;

// ── Server-only re-exports ───────────────────────────────────────────

#[cfg(feature = "server")]
pub use cache::TokenCache;
#[cfg(feature = "server")]
pub use get::{
    check_token_limit, get_token, get_token_by_key_or_username, parse_token_limit_with_compat,
};
#[cfg(feature = "server")]
pub use super_token::check_super_token;

#[cfg(feature = "server")]
mod auth_checker_impl;

// ── Shared hashing utilities (server only, used by multiple modules) ─

#[cfg(feature = "server")]
use sha2::{Digest, Sha256};

/// Hash a string with the NODEGET salt, returning hex-encoded result.
#[cfg(feature = "server")]
pub fn hash_string(need_hash: &str) -> String {
    let bytes = hash_to_bytes(need_hash);
    hex::encode(bytes)
}

/// Hash a string with the NODEGET salt, returning raw 32-byte digest.
#[cfg(feature = "server")]
pub fn hash_to_bytes(need_hash: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"NODEGET");
    hasher.update(need_hash.as_bytes());
    hasher.finalize().into()
}

// ── AuthChecker integration point ──────────────────────────────────

/// Register this crate's `AuthChecker` implementation with ng-infra's global.
///
/// Must be called once during server startup, after `TokenCache::init()`.
#[cfg(feature = "server")]
pub fn register_auth_checker() {
    ng_infra::server::set_auth_checker(Box::new(auth_checker_impl::TokenAuthChecker));
}

/// Build and return the token RPC module.
///
/// The caller should merge this into the main RPC module during startup:
/// ```ignore
/// main_module.merge(ng_token::rpc_module()).unwrap();
/// ```
#[cfg(feature = "server")]
pub fn rpc_module() -> jsonrpsee::RpcModule<rpc::TokenRpcImpl> {
    use rpc::RpcServer;
    rpc::TokenRpcImpl.into_rpc()
}
