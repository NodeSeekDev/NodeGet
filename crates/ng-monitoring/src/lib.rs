//! ng-monitoring：监控数据结构、缓存与 RPC 方法。
//!
//! ## 默认 feature（仅类型，Agent 端可安全依赖）
//! - `StaticMonitoringData`、`DynamicMonitoringData`、`DynamicMonitoringSummaryData`
//! - 查询 DSL（`QueryCondition`、`StaticDataQuery`、`DynamicDataQuery`、`DynamicSummaryQuery` 等）
//! - 响应类型（`StaticResponseItem`、`DynamicResponseItem`、`DynamicSummaryResponseItem`）
//! - 反缩放工具（`apply_descaling_to_json_object`、`SCALED_SUMMARY_COLUMNS`）
//!
//! ## `server` feature（仅 Server 端启用）
//! - `MonitoringBuffer` — 监控数据批量写入缓冲区
//! - `MonitoringUuidCache` — 基于 DB 的 UUID↔ID 双向缓存
//! - `MonitoringLastCache` — 内存中的最新值缓存
//! - `StaticHashCache` — 内存中的静态数据哈希去重缓存
//! - RPC 命名空间：`agent`、`agent-uuid`、`nodeget-server`
//! - `rpc_module()` — 合并所有监控相关 RPC 方法的统一入口

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

/// 构建并返回包含所有监控相关 RPC 方法的 `RpcModule`。
///
/// 合并了 `agent`、`agent-uuid`、`nodeget-server` 三个命名空间。
/// 调用方应在启动时将此模块 merge 到主 RPC 模块中。
///
/// - 合并 `AgentRpcImpl`（agent 命名空间）
/// - 合并 `AgentUuidRpcImpl`（agent-uuid 命名空间）
/// - 合并 `NodegetServerRpcImpl`（nodeget-server 命名空间）
///
/// # Panics
///
/// 若各 RPC 模块 `merge` 失败则 panic（配置正确时不应发生）。
#[cfg(feature = "server")]
#[must_use]
pub fn rpc_module() -> jsonrpsee::RpcModule<()> {
    use rpc::agent::RpcServer as AgentRpcServer;
    use rpc::agent_uuid::AgentUuidRpcServer;
    use rpc::nodeget::RpcServer as NodegetServerRpcServer;

    let mut module = jsonrpsee::RpcModule::new(());

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
