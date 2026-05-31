use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod create;
mod delete;
mod list;
mod read;
mod update;

#[rpc(server, namespace = "static-bucket")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(
        &self,
        token: String,
        name: String,
        path: String,
        is_http_root: bool,
        cors: bool,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "read")]
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "update")]
    async fn update(
        &self,
        token: String,
        name: String,
        path: String,
        is_http_root: bool,
        cors: bool,
        enable: Option<bool>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "list")]
    async fn list(&self, token: String) -> RpcResult<Box<RawValue>>;
}

pub struct StaticBucketRpcImpl;

impl RpcHelper for StaticBucketRpcImpl {}

#[async_trait]
impl RpcServer for StaticBucketRpcImpl {
    async fn create(
        &self,
        token: String,
        name: String,
        path: String,
        is_http_root: bool,
        cors: bool,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket", "static-bucket::create", token_key = tk, username = un, name = %name, path = %path, is_http_root = is_http_root, cors = cors);
        async { rpc_exec!(create::create(token, name, path, is_http_root, cors).await) }
            .instrument(span)
            .await
    }

    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket", "static-bucket::read", token_key = tk, username = un, name = %name);
        async { rpc_exec!(read::read(token, name).await) }
            .instrument(span)
            .await
    }

    async fn update(
        &self,
        token: String,
        name: String,
        path: String,
        is_http_root: bool,
        cors: bool,
        enable: Option<bool>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket", "static-bucket::update", token_key = tk, username = un, name = %name, path = %path, is_http_root = is_http_root, cors = cors, enable = ?enable);
        async { rpc_exec!(update::update(token, name, path, is_http_root, cors, enable).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket", "static-bucket::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    async fn list(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket", "static-bucket::list", token_key = tk, username = un);
        async { rpc_exec!(list::list_rpc(token).await) }
            .instrument(span)
            .await
    }
}
