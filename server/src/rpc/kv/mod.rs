use crate::rpc::RpcHelper;
use crate::rpc::{rpc_exec, token_identity};
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod create;
mod delete_key;
mod delete_namespace;
mod get_all_keys;
mod get_multi_value;
mod get_value;
mod list_all_namespace;
mod set_value;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamespaceKeyItem {
    pub namespace: String,
    pub key: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KvValueItem {
    pub namespace: String,
    pub key: String,
    pub value: Value,
}

#[rpc(server, namespace = "kv")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(&self, token: String, namespace: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "get_value")]
    async fn get_value(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "get_multi_value")]
    async fn get_multi_value(
        &self,
        token: String,
        namespace_key: Vec<NamespaceKeyItem>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "set_value")]
    async fn set_value(
        &self,
        token: String,
        namespace: String,
        key: String,
        value: Value,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete_key")]
    async fn delete_key(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete_namespace")]
    async fn delete_namespace(
        &self,
        token: String,
        namespace: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "get_all_keys")]
    async fn get_all_keys(&self, token: String, namespace: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "list_all_namespace")]
    async fn list_all_namespace(&self, token: String) -> RpcResult<Box<RawValue>>;
}

pub struct KvRpcImpl;

impl RpcHelper for KvRpcImpl {}

#[async_trait]
impl RpcServer for KvRpcImpl {
    async fn create(&self, token: String, namespace: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::create", token_key = tk, username = un, namespace = %namespace);
        async { rpc_exec!(create::create(token, namespace).await) }
            .instrument(span)
            .await
    }

    async fn get_value(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::get_value", token_key = tk, username = un, namespace = %namespace, key = %key);
        async { rpc_exec!(get_value::get_value(token, namespace, key).await) }
            .instrument(span)
            .await
    }

    async fn get_multi_value(
        &self,
        token: String,
        namespace_key: Vec<NamespaceKeyItem>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::get_multi_value", token_key = tk, username = un, items_count = namespace_key.len());
        async { rpc_exec!(get_multi_value::get_multi_value(token, namespace_key).await) }
            .instrument(span)
            .await
    }

    async fn set_value(
        &self,
        token: String,
        namespace: String,
        key: String,
        value: Value,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::set_value", token_key = tk, username = un, namespace = %namespace, key = %key);
        async { rpc_exec!(set_value::set_value(token, namespace, key, value).await) }
            .instrument(span)
            .await
    }

    async fn delete_key(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::delete_key", token_key = tk, username = un, namespace = %namespace, key = %key);
        async { rpc_exec!(delete_key::delete_key(token, namespace, key).await) }
            .instrument(span)
            .await
    }

    async fn delete_namespace(
        &self,
        token: String,
        namespace: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::delete_namespace", token_key = tk, username = un, namespace = %namespace);
        async { rpc_exec!(delete_namespace::delete_namespace(token, namespace).await) }
            .instrument(span)
            .await
    }

    async fn get_all_keys(&self, token: String, namespace: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::get_all_keys", token_key = tk, username = un, namespace = %namespace);
        async { rpc_exec!(get_all_keys::get_all_keys(token, namespace).await) }
            .instrument(span)
            .await
    }

    async fn list_all_namespace(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::list_all_namespace", token_key = tk, username = un);
        async { rpc_exec!(list_all_namespace::list_all_namespace(token).await) }
            .instrument(span)
            .await
    }
}
