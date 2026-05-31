//! ng-kv: KV store types and server-side KV namespace management.
//!
//! ## Default features (types only)
//! - [`KVStore`] — namespace-scoped key-value store with JSON values
//!
//! ## `server` feature
//! - DB read/write operations for KV namespaces
//! - KV RPC namespace (JSON-RPC methods)
//! - Permission filtering for KV operations
//! - [`TokenPermissionChecker`] trait + global injection for auth delegation

mod kv;
pub use kv::KVStore;

#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
mod db;
#[cfg(feature = "server")]
pub mod rpc;

#[cfg(feature = "server")]
pub use auth::{
    KvNamespaceListPermission, TokenPermissionChecker, check_kv_create_permission,
    check_kv_delete_namespace_permission, check_kv_delete_permission,
    check_kv_list_keys_permission, check_kv_read_permission, check_kv_read_permission_with_pattern,
    check_kv_write_permission, get_token_checker, resolve_kv_list_namespace_permission,
    set_token_checker, validate_key, validate_key_pattern,
};

#[cfg(feature = "server")]
pub use db::{
    NAMESPACE_MARKER_KEY, create_kv, delete_key_from_kv, delete_kv, get_keys_from_kv, get_kv_store,
    get_kv_store_optional, get_or_create_kv, get_v_from_kv, list_all_namespaces, set_v_to_kv,
};

#[cfg(feature = "server")]
pub use rpc::{KvValueItem, NamespaceKeyItem, rpc_module};
