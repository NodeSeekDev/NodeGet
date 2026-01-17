mod query;
mod report;

use crate::DB;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use sea_orm::{
    ActiveValue, DatabaseConnection,
    Set,
};
use serde::Serialize;
use serde_json::{Value, to_value};

#[rpc(server, namespace = "agent")]
pub trait Rpc {
    #[method(name = "report_static")]
    async fn report_static(&self, token: String, data: Value) -> Value;

    #[method(name = "report_dynamic")]
    async fn report_dynamic(&self, token: String, data: Value) -> Value;

    #[method(name = "query_static")]
    async fn query_static(&self, token: String, data: Value) -> Value;

    #[method(name = "query_dynamic")]
    async fn query_dynamic(&self, token: String, data: Value) -> Value;
}
pub struct AgentRpcImpl;

impl AgentRpcImpl {
    fn try_set_json<T: Serialize>(val: T) -> Result<ActiveValue<Value>, String> {
        to_value(val)
            .map(Set)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    fn get_db() -> Result<&'static DatabaseConnection, (i64, String)> {
        DB.get()
            .ok_or_else(|| (102, "DB not initialized".to_string()))
    }
}

#[async_trait]
impl RpcServer for AgentRpcImpl {
    async fn report_static(&self, token: String, data: Value) -> Value {
        report::report_static(token, data).await
    }

    async fn report_dynamic(&self, token: String, data: Value) -> Value {
        report::report_dynamic(token, data).await
    }

    async fn query_static(&self, token: String, data: Value) -> Value {
        query::query_static(token, data).await
    }

    async fn query_dynamic(&self, token: String, data: Value) -> Value {
        query::query_dynamic(token, data).await
    }
}
