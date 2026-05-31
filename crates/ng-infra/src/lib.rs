//! ng-infra: Infrastructure traits and types for NodeGet.
//!
//! This crate provides shared infrastructure that both the server and agent
//! can depend on, without pulling in heavy dependencies like jsonrpsee or sea-orm.
//!
//! ## Default features (types only)
//! - [`ScopedPermission<T>`] — permission scope restriction enum
//! - [`PermissionResolver`] — trait for resolving permissions
//! - [`RpcDispatcher`] — trait for RPC method dispatch
//!
//! ## `server` feature
//! - [`DbBackedCache`] trait + [`make_global_cache!`] macro
//! - [`rpc_exec!`] macro
//! - [`TruncatedRaw`] — truncated Display wrapper for RawValue
//! - [`RpcHelper`] trait — DB and serialization utilities
//! - [`token_identity`] — token string parser
//! - [`AuthChecker`] trait + global injection

pub mod dispatcher;
pub mod permission;

#[cfg(feature = "server")]
pub mod server;

pub use dispatcher::RpcDispatcher;
pub use permission::{PermissionResolver, ScopedPermission};
