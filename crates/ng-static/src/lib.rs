//! ng-static: Static file bucket management for NodeGet.
//!
//! ## Default features (types only)
//! - [`FileInfo`] — metadata for a file within a static bucket
//!
//! ## `server` feature
//! - [`StaticCache`] — DB-backed in-memory static bucket cache
//! - Bucket CRUD (`create_static`, `read_static`, `update_static`, `delete_static`)
//! - File operations (`upload_file`, `read_file`, `delete_file`, `rename_file`, `list_file`)
//! - Path safety (`validate_name`, `validate_sub_path`, `resolve_safe_file_path`)
//! - `static-bucket` RPC namespace
//! - `static-bucket-file` RPC namespace
//! - [`router()`] — axum Router for `/nodeget/static/{name}` + WebDAV routes
//! - [`rpc_module()`] — JSON-RPC module containing both namespaces

// ── Types always available ──────────────────────────────────────────

/// Metadata for a single file within a static bucket.
///
/// Returned by the `static-bucket-file.list` RPC method.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileInfo {
    /// Relative path under `{static_path}/{sub_path}/`, using `/` separator.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Last modified time: Unix millisecond timestamp; `0` if unavailable.
    pub mtime: i64,
}

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod auth;
#[cfg(feature = "server")]
pub mod cache;
#[cfg(feature = "server")]
pub mod ops;
#[cfg(feature = "server")]
pub mod router;
#[cfg(feature = "server")]
pub mod rpc;

// ── Server-only re-exports ───────────────────────────────────────────

#[cfg(feature = "server")]
pub use cache::StaticCache;
#[cfg(feature = "server")]
pub use ops::{
    create_static, delete_file, delete_static, get_static_path, list_all_names, list_file,
    read_file, read_static, rename_file, resolve_safe_file_path, update_static, upload_file,
    validate_name, validate_sub_path,
};
