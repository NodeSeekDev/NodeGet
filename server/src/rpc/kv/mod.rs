use crate::rpc::RpcHelper;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use serde_json::Value;
use serde_json::value::RawValue;

mod auth;
mod create;
mod delete_key;
mod get_all_keys;
mod get_value;
mod list_all_namespace;
mod set_value;

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
        create::create(token, namespace).await
    }

    async fn get_value(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>> {
        get_value::get_value(token, namespace, key).await
    }

    async fn set_value(
        &self,
        token: String,
        namespace: String,
        key: String,
        value: Value,
    ) -> RpcResult<Box<RawValue>> {
        set_value::set_value(token, namespace, key, value).await
    }

    async fn delete_key(
        &self,
        token: String,
        namespace: String,
        key: String,
    ) -> RpcResult<Box<RawValue>> {
        delete_key::delete_key(token, namespace, key).await
    }

    async fn get_all_keys(&self, token: String, namespace: String) -> RpcResult<Box<RawValue>> {
        get_all_keys::get_all_keys(token, namespace).await
    }

    async fn list_all_namespace(&self, token: String) -> RpcResult<Box<RawValue>> {
        list_all_namespace::list_all_namespace(token).await
    }
}
