use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod delete_file;
mod list_file;
mod read_file;
mod rename_file;
mod upload_file;

#[rpc(server, namespace = "static-bucket-file")]
pub trait Rpc {
    #[method(name = "upload")]
    async fn upload(
        &self,
        token: String,
        name: String,
        path: String,
        body: Option<Vec<u8>>,
        base64: Option<String>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "read")]
    async fn read(&self, token: String, name: String, path: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String, path: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "rename")]
    async fn rename(
        &self,
        token: String,
        name: String,
        from: String,
        to: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "list")]
    async fn list(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;
}

pub struct StaticBucketFileRpcImpl;

impl RpcHelper for StaticBucketFileRpcImpl {}

#[async_trait]
impl RpcServer for StaticBucketFileRpcImpl {
    async fn upload(
        &self,
        token: String,
        name: String,
        path: String,
        body: Option<Vec<u8>>,
        base64: Option<String>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket_file", "static-bucket-file::upload", token_key = tk, username = un, name = %name, path = %path, has_body = body.is_some(), has_base64 = base64.is_some());
        async { rpc_exec!(upload_file::upload_file_rpc(token, name, path, body, base64).await) }
            .instrument(span)
            .await
    }

    async fn read(&self, token: String, name: String, path: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket_file", "static-bucket-file::read", token_key = tk, username = un, name = %name, path = %path);
        async { rpc_exec!(read_file::read_file_rpc(token, name, path).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, name: String, path: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket_file", "static-bucket-file::delete", token_key = tk, username = un, name = %name, path = %path);
        async { rpc_exec!(delete_file::delete_file_rpc(token, name, path).await) }
            .instrument(span)
            .await
    }

    async fn rename(
        &self,
        token: String,
        name: String,
        from: String,
        to: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket_file", "static-bucket-file::rename", token_key = tk, username = un, name = %name, from = %from, to = %to);
        async { rpc_exec!(rename_file::rename_file_rpc(token, name, from, to).await) }
            .instrument(span)
            .await
    }

    async fn list(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "static_bucket_file", "static-bucket-file::list", token_key = tk, username = un, name = %name);
        async { rpc_exec!(list_file::list_file_rpc(token, name).await) }
            .instrument(span)
            .await
    }
}
