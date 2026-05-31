use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_core::js_result::query::JsResultDataQuery;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use serde_json::value::RawValue;
use tracing::Instrument;

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
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_result", "js-result::query", token_key = tk, username = un, query = ?query);
        async { rpc_exec!(query::query(token, query).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_result", "js-result::delete", token_key = tk, username = un, query = ?query);
        async { rpc_exec!(delete::delete(token, query).await) }
            .instrument(span)
            .await
    }
}

/// Build and return an [`jsonrpsee::RpcModule`] with all js-result RPC methods registered.
pub fn rpc_module() -> jsonrpsee::RpcModule<JsResultRpcImpl> {
    JsResultRpcImpl.into_rpc()
}
