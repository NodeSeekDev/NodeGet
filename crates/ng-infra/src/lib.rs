//! ng-infra：NodeGet 基础设施层，提供 Server 与 Agent 共用的 trait 和类型。
//!
//! 本 crate 不引入 jsonrpsee、sea-orm 等重依赖，Agent 可安全依赖。
//!
//! ## 默认 feature（仅类型）
//! - [`ScopedPermission<T>`] — 权限范围限制枚举
//! - [`PermissionResolver`] — 权限解析 trait
//! - [`RpcDispatcher`] — RPC 方法派发 trait
//!
//! ## `server` feature
//! - [`DbBackedCache`] trait + [`make_global_cache!`] 宏 — DB 全量加载缓存
//! - [`rpc_exec!`] 宏 — RPC 调用统一日志
//! - [`TruncatedRaw`] — RawValue 截断 Display 包装
//! - [`RpcHelper`] trait — DB 连接与序列化工具
//! - [`token_identity`] — Token 字符串解析

pub mod dispatcher;
pub mod permission;

#[cfg(feature = "server")]
pub mod server;

pub use dispatcher::RpcDispatcher;
pub use permission::{PermissionResolver, ScopedPermission};
