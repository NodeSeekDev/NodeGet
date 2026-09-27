//! `agent` RPC 命名空间实现。
//!
//! 提供 Agent 监控数据的上报（report）、查询（query）、删除（delete）操作，
//! 涵盖三类监控数据：`static`、`dynamic`、`dynamic_summary`。
//! 所有方法均通过 `rpc_exec!` 宏统一日志和错误处理。

use crate::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use crate::query::{
    DynamicDataQuery, DynamicDataQueryField, DynamicSummaryQuery, DynamicSummaryQueryField,
    QueryCondition, StaticDataQuery, StaticDataQueryField, TrafficQuery,
};
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::RpcHelper;
use ng_infra::server::token_identity;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

mod delete_common;
mod delete_dynamic;
mod delete_dynamic_summary;
mod delete_static;
mod query_dynamic;
mod query_dynamic_multi_last;
pub mod query_dynamic_summary;
mod query_dynamic_summary_multi_last;
mod query_static;
mod query_static_multi_last;
mod query_traffic;
mod report_dynamic;
mod report_dynamic_summary;
mod report_static;

/// `agent` RPC trait 定义，使用 `#[rpc]` 宏自动生成 server 端骨架。
#[rpc(server, namespace = "agent")]
pub trait Rpc {
    /// Agent 上报静态监控数据
    #[method(name = "report_static")]
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> RpcResult<Box<RawValue>>;

    /// Agent 上报动态监控数据
    #[method(name = "report_dynamic")]
    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> RpcResult<Box<RawValue>>;

    /// 查询静态监控数据
    #[method(name = "query_static")]
    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>>;

    /// 查询动态监控数据
    #[method(name = "query_dynamic")]
    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>>;

    /// 批量查询多台设备的静态最新值
    #[method(name = "static_data_multi_last_query")]
    async fn static_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<StaticDataQueryField>,
    ) -> RpcResult<Box<RawValue>>;

    /// 批量查询多台设备的动态最新值
    #[method(name = "dynamic_data_multi_last_query")]
    async fn dynamic_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicDataQueryField>,
    ) -> RpcResult<Box<RawValue>>;

    /// 删除静态监控数据
    #[method(name = "delete_static")]
    async fn delete_static(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>>;

    /// 删除动态监控数据
    #[method(name = "delete_dynamic")]
    async fn delete_dynamic(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>>;

    /// Agent 上报动态摘要监控数据
    #[method(name = "report_dynamic_summary")]
    async fn report_dynamic_summary(
        &self,
        token: String,
        data: DynamicMonitoringSummaryData,
    ) -> RpcResult<Box<RawValue>>;

    /// 查询动态摘要监控数据
    #[method(name = "query_dynamic_summary")]
    async fn query_dynamic_summary(
        &self,
        token: String,
        query: DynamicSummaryQuery,
    ) -> RpcResult<Box<RawValue>>;

    /// 批量查询多台设备的动态摘要最新值
    #[method(name = "dynamic_summary_multi_last_query")]
    async fn dynamic_summary_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicSummaryQueryField>,
    ) -> RpcResult<Box<RawValue>>;

    /// 删除动态摘要监控数据
    #[method(name = "delete_dynamic_summary")]
    async fn delete_dynamic_summary(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>>;

    /// 查询流量统计
    #[method(name = "query_traffic")]
    async fn query_traffic(&self, token: String, query: TrafficQuery) -> RpcResult<Box<RawValue>>;
}

/// `agent` RPC 实现，委托给各子模块的具体函数。
pub struct AgentRpcImpl;

impl RpcHelper for AgentRpcImpl {}

#[async_trait]
impl RpcServer for AgentRpcImpl {
    /// 上报静态监控数据，委托给 `report_static` 模块。
    ///
    /// 1. 提取 Token 身份信息
    /// 2. 创建 tracing span
    /// 3. 通过 `rpc_exec!` 调用 `report_static::report_static`
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::report_static", token_key = tk, username = un, uuid = %static_monitoring_data.uuid);
        async { rpc_exec!(report_static::report_static(token, static_monitoring_data).await) }
            .instrument(span)
            .await
    }

    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::report_dynamic", token_key = tk, username = un, uuid = %dynamic_monitoring_data.uuid);
        async { rpc_exec!(report_dynamic::report_dynamic(token, dynamic_monitoring_data).await) }
            .instrument(span)
            .await
    }

    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::query_static", token_key = tk, username = un, query = ?static_data_query);
        async { rpc_exec!(query_static::query_static(token, static_data_query).await) }
            .instrument(span)
            .await
    }

    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::query_dynamic", token_key = tk, username = un, query = ?dynamic_data_query);
        async { rpc_exec!(query_dynamic::query_dynamic(token, dynamic_data_query).await) }
            .instrument(span)
            .await
    }

    async fn static_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<StaticDataQueryField>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::static_data_multi_last_query", token_key = tk, username = un, uuids = ?uuids, fields = ?fields);
        async {
            rpc_exec!(
                query_static_multi_last::static_data_multi_last_query(token, uuids, fields).await
            )
        }
        .instrument(span)
        .await
    }

    async fn dynamic_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicDataQueryField>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::dynamic_data_multi_last_query", token_key = tk, username = un, uuids = ?uuids, fields = ?fields);
        async {
            rpc_exec!(
                query_dynamic_multi_last::dynamic_data_multi_last_query(token, uuids, fields).await
            )
        }
        .instrument(span)
        .await
    }

    async fn delete_static(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::delete_static", token_key = tk, username = un, conditions = ?conditions);
        async { rpc_exec!(delete_static::delete_static(token, conditions).await) }
            .instrument(span)
            .await
    }

    async fn delete_dynamic(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::delete_dynamic", token_key = tk, username = un, conditions = ?conditions);
        async { rpc_exec!(delete_dynamic::delete_dynamic(token, conditions).await) }
            .instrument(span)
            .await
    }

    async fn report_dynamic_summary(
        &self,
        token: String,
        data: DynamicMonitoringSummaryData,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::report_dynamic_summary", token_key = tk, username = un, uuid = %data.uuid);
        async { rpc_exec!(report_dynamic_summary::report_dynamic_summary(token, data).await) }
            .instrument(span)
            .await
    }

    async fn query_dynamic_summary(
        &self,
        token: String,
        query: DynamicSummaryQuery,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::query_dynamic_summary", token_key = tk, username = un, query = ?query);
        async { rpc_exec!(query_dynamic_summary::query_dynamic_summary(token, query).await) }
            .instrument(span)
            .await
    }

    async fn dynamic_summary_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicSummaryQueryField>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::dynamic_summary_multi_last_query", token_key = tk, username = un, uuids = ?uuids, fields = ?fields);
        async {
            rpc_exec!(
                query_dynamic_summary_multi_last::dynamic_summary_multi_last_query(
                    token, uuids, fields
                )
                .await
            )
        }
        .instrument(span)
        .await
    }

    async fn delete_dynamic_summary(
        &self,
        token: String,
        conditions: Vec<QueryCondition>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::delete_dynamic_summary", token_key = tk, username = un, conditions = ?conditions);
        async { rpc_exec!(delete_dynamic_summary::delete_dynamic_summary(token, conditions).await) }
            .instrument(span)
            .await
    }

    async fn query_traffic(&self, token: String, query: TrafficQuery) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "monitoring", "agent::query_traffic", token_key = tk, username = un, query = ?query);
        async { rpc_exec!(query_traffic::query_traffic(token, query).await) }
            .instrument(span)
            .await
    }
}
