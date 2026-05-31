//! ng-monitoring: Monitoring data structures, caches, and RPC methods for `NodeGet`.
//!
//! ## Default features (types only)
//! - `StaticMonitoringData`, `DynamicMonitoringData`, `DynamicMonitoringSummaryData`
//! - Query DSL (`QueryCondition`, `StaticDataQuery`, `DynamicDataQuery`, `DynamicSummaryQuery`, etc.)
//! - Response types (`StaticResponseItem`, `DynamicResponseItem`, `DynamicSummaryResponseItem`)
//! - Descaling helpers (`apply_descaling_to_json_object`, `SCALED_SUMMARY_COLUMNS`)
//!
//! ## `server` feature
//! - `MonitoringBuffer` — batched write buffer for monitoring data
//! - `MonitoringUuidCache` — DB-backed UUID-to-ID cache
//! - `MonitoringLastCache` — in-memory last-value cache
//! - `StaticHashCache` — in-memory static data hash dedup cache
//! - RPC namespaces: `agent`, `agent-uuid`, `nodeget-server`
//! - `rpc_module()` — merged RPC module for all monitoring-related methods

#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

// ── Default (types only) ────────────────────────────────────────────

pub mod data_structure;
pub mod query;

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod monitoring_buffer;
#[cfg(feature = "server")]
pub mod monitoring_last_cache;
#[cfg(feature = "server")]
pub mod monitoring_uuid_cache;
#[cfg(feature = "server")]
pub mod rpc;
#[cfg(feature = "server")]
pub mod static_hash_cache;

/// Build and return an `RpcModule` containing all monitoring-related RPC methods
/// (agent + agent-uuid + nodeget-server).
///
/// The caller should merge this into the main RPC module during startup.
#[cfg(feature = "server")]
#[must_use]
pub fn rpc_module() -> jsonrpsee::RpcModule<()> {
    use rpc::agent::RpcServer as AgentRpcServer;
    use rpc::agent_uuid::AgentUuidRpcServer;
    use rpc::nodeget::RpcServer as NodegetServerRpcServer;

    let mut module = jsonrpsee::RpcModule::new(());

    module
        .register_method("agent_ping", |_, (), _| {
            Ok::<&str, jsonrpsee::types::ErrorObjectOwned>("pong")
        })
        .ok();

    let agent_impl = rpc::agent::AgentRpcImpl;
    module
        .merge(agent_impl.into_rpc())
        .expect("merge agent rpc");

    let agent_uuid_impl = rpc::agent_uuid::AgentUuidRpcImpl;
    module
        .merge(agent_uuid_impl.into_rpc())
        .expect("merge agent-uuid rpc");

    let nodeget_impl = rpc::nodeget::NodegetServerRpcImpl;
    module
        .merge(nodeget_impl.into_rpc())
        .expect("merge nodeget-server rpc from ng-monitoring");

    module
}
