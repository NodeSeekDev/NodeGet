// Agent 是 Server Rpc 功能模板，开发请按照本模板进行
// 该文件仅定义，不实现

// Agent 静态监控数据查询模块
mod query;
// Agent 监控数据上报模块
mod report;

use crate::rpc::RpcHelper;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use nodeget_lib::monitoring::query::{DynamicDataQuery, StaticDataQuery};
use serde_json::Value;
use serde_json::value::RawValue;

// Agent 相关的 RPC 接口定义，包括静态和动态监控数据的上报与查询功能
#[rpc(server, namespace = "agent")]
pub trait Rpc {
    // 上报静态监控数据方法
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `static_monitoring_data` - 静态监控数据
    // 
    // # 返回值
    // 返回操作结果
    #[method(name = "report_static")]
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> Value;

    // 上报动态监控数据方法
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `dynamic_monitoring_data` - 动态监控数据
    // 
    // # 返回值
    // 返回操作结果
    #[method(name = "report_dynamic")]
    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> Value;

    // 查询静态监控数据方法
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `static_data_query` - 静态数据查询条件
    // 
    // # 返回值
    // 返回查询结果，格式为 Vec<StaticResponseItem>
    #[method(name = "query_static")]
    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>>; // Vec<StaticResponseItem>

    // 查询动态监控数据方法
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `dynamic_data_query` - 动态数据查询条件
    // 
    // # 返回值
    // 返回查询结果，格式为 Vec<DynamicResponseItem>
    #[method(name = "query_dynamic")]
    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>>; // Vec<DynamicResponseItem>
}
// Agent RPC 实现结构体
pub struct AgentRpcImpl;

// 为 AgentRpcImpl 实现 RPC 辅助功能
impl RpcHelper for AgentRpcImpl {}

#[async_trait]
impl RpcServer for AgentRpcImpl {
    // 上报静态监控数据实现
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `static_monitoring_data` - 静态监控数据
    // 
    // # 返回值
    // 返回操作结果
    async fn report_static(
        &self,
        token: String,
        static_monitoring_data: StaticMonitoringData,
    ) -> Value {
        report::report_static(token, static_monitoring_data).await
    }

    // 上报动态监控数据实现
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `dynamic_monitoring_data` - 动态监控数据
    // 
    // # 返回值
    // 返回操作结果
    async fn report_dynamic(
        &self,
        token: String,
        dynamic_monitoring_data: DynamicMonitoringData,
    ) -> Value {
        report::report_dynamic(token, dynamic_monitoring_data).await
    }

    // 查询静态监控数据实现
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `static_data_query` - 静态数据查询条件
    // 
    // # 返回值
    // 返回查询结果，格式为 Vec<StaticResponseItem>
    async fn query_static(
        &self,
        token: String,
        static_data_query: StaticDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query::query_static(token, static_data_query).await
    }

    // 查询动态监控数据实现
    // 
    // # 参数
    // * `token` - 认证令牌
    // * `dynamic_data_query` - 动态数据查询条件
    // 
    // # 返回值
    // 返回查询结果，格式为 Vec<DynamicResponseItem>
    async fn query_dynamic(
        &self,
        token: String,
        dynamic_data_query: DynamicDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query::query_dynamic(token, dynamic_data_query).await
    }
}
