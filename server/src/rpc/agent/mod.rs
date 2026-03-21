use crate::rpc::RpcHelper;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use nodeget_lib::monitoring::query::{
    DynamicDataAvgQuery, DynamicDataQuery, DynamicDataQueryField, StaticDataAvgQuery,
    StaticDataQuery, StaticDataQueryField,
};
use serde_json::value::RawValue;
use uuid::Uuid;

mod query_dynamic_avg;
mod query_dynamic;
mod query_dynamic_multi_last;
mod delete_dynamic;
mod delete_static;
mod query_static_avg;
mod query_static;
mod query_static_multi_last;
mod report_dynamic;
mod report_static;

#[rpc(server, namespace = "agent")]
pub trait Rpc {
    #[method(name = "report_static")]
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "report_dynamic")]
    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "query_static")]
    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "query_dynamic")]
    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "query_static_avg")]
    async fn query_static_avg(
        &self,
        token: String,
        static_data_avg_query: StaticDataAvgQuery,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "query_dynamic_avg")]
    async fn query_dynamic_avg(
        &self,
        token: String,
        dynamic_data_avg_query: DynamicDataAvgQuery,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "static_data_multi_last_query")]
    async fn static_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<StaticDataQueryField>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "dynamic_data_multi_last_query")]
    async fn dynamic_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicDataQueryField>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete_static")]
    async fn delete_static(
        &self,
        token: String,
        agent_uuid: Uuid,
        before_timestamp: i64,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete_dynamic")]
    async fn delete_dynamic(
        &self,
        token: String,
        agent_uuid: Uuid,
        before_timestamp: i64,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct AgentRpcImpl;

impl RpcHelper for AgentRpcImpl {}

#[async_trait]
impl RpcServer for AgentRpcImpl {
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> RpcResult<Box<RawValue>> {
        report_static::report_static(token, static_monitoring_data).await
    }

    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> RpcResult<Box<RawValue>> {
        report_dynamic::report_dynamic(token, dynamic_monitoring_data).await
    }

    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query_static::query_static(token, static_data_query).await
    }

    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query_dynamic::query_dynamic(token, dynamic_data_query).await
    }

    async fn query_static_avg(
        &self,
        token: String,
        static_data_avg_query: StaticDataAvgQuery,
    ) -> RpcResult<Box<RawValue>> {
        query_static_avg::query_static_avg(token, static_data_avg_query).await
    }

    async fn query_dynamic_avg(
        &self,
        token: String,
        dynamic_data_avg_query: DynamicDataAvgQuery,
    ) -> RpcResult<Box<RawValue>> {
        query_dynamic_avg::query_dynamic_avg(token, dynamic_data_avg_query).await
    }

    async fn static_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<StaticDataQueryField>,
    ) -> RpcResult<Box<RawValue>> {
        query_static_multi_last::static_data_multi_last_query(token, uuids, fields).await
    }

    async fn dynamic_data_multi_last_query(
        &self,
        token: String,
        uuids: Vec<Uuid>,
        fields: Vec<DynamicDataQueryField>,
    ) -> RpcResult<Box<RawValue>> {
        query_dynamic_multi_last::dynamic_data_multi_last_query(token, uuids, fields).await
    }

    async fn delete_static(
        &self,
        token: String,
        agent_uuid: Uuid,
        before_timestamp: i64,
    ) -> RpcResult<Box<RawValue>> {
        delete_static::delete_static(token, agent_uuid, before_timestamp).await
    }

    async fn delete_dynamic(
        &self,
        token: String,
        agent_uuid: Uuid,
        before_timestamp: i64,
    ) -> RpcResult<Box<RawValue>> {
        delete_dynamic::delete_dynamic(token, agent_uuid, before_timestamp).await
    }
}
