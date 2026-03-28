use crate::rpc::RpcHelper;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::js_result::query::JsResultDataQuery;
use serde_json::value::RawValue;

mod delete;
mod query;

#[rpc(server, namespace = "js-result")]
pub trait Rpc {
    #[method(name = "query")]
    async fn query(&self, token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>>;
}

pub struct JsResultRpcImpl;

impl RpcHelper for JsResultRpcImpl {}

#[async_trait]
impl RpcServer for JsResultRpcImpl {
    async fn query(&self, token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
        query::query(token, query).await
    }

    async fn delete(&self, token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
        delete::delete(token, query).await
    }
}
