//! ng-task: Task types and RPC namespace for NodeGet.
//!
//! ## Default features (types only)
//! - `TaskEventType`, `TaskEvent`, `TaskEventResult` — task type definitions
//! - `TaskEventResponse` — task result upload structure
//! - `WebShellTask`, `ExecuteTask`, `HttpRequestTask`, `DnsTask` — parameter types
//! - `DnsRecordResult`, `HttpRequestTaskResult` — result types
//! - `query` module — `TaskQueryCondition`, `TaskDataQuery`, `TaskResponseItem`
//!
//! ## `server` feature
//! - `TaskManager` — broadcast channel manager for task events
//! - Task RPC namespace — `task.*` JSON-RPC methods
//! - `TaskAuthProvider` — trait for auth checking (injected by server)
//! - `MonitoringUuidProvider` — trait for UUID cache operations (injected by server)

pub mod types;

// Re-export types at crate root for convenience
pub use types::query;
pub use types::{
    DnsRecordResult, DnsRecordType, DnsTask, ExecuteTask, HttpRequestTask, HttpRequestTaskResult,
    TaskEvent, TaskEventResponse, TaskEventResult, TaskEventType, WebShellTask,
};

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod rpc;

#[cfg(feature = "server")]
pub use rpc::{
    MonitoringUuidProvider, TaskAuthProvider, TaskManager, auth_provider, monitoring_uuid_provider,
    rpc_module, set_auth_provider, set_monitoring_uuid_provider,
};
