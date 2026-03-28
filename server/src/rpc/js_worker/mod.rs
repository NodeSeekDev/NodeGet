use crate::rpc::RpcHelper;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use serde_json::Value;
use serde_json::value::RawValue;

mod auth;
mod create;
mod delete;
mod get_rt_pool;
mod list_all_js_worker;
mod read;
mod run;
pub(crate) mod service;
mod update;

#[rpc(server, namespace = "js-worker")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(
        &self,
        token: String,
        name: String,
        js_script_base64: String,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "update")]
    async fn update(
        &self,
        token: String,
        name: String,
        js_script_base64: String,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "read")]
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "run")]
    async fn run(
        &self,
        token: String,
        js_script_name: String,
        run_type: Option<nodeget_lib::js_runtime::RunType>,
        params: Value,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "get_rt_pool")]
    async fn get_rt_pool(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "list_all_js_worker")]
    async fn list_all_js_worker(&self, token: String) -> RpcResult<Box<RawValue>>;
}

pub struct JsWorkerRpcImpl;

impl RpcHelper for JsWorkerRpcImpl {}

#[async_trait]
impl RpcServer for JsWorkerRpcImpl {
    async fn create(
        &self,
        token: String,
        name: String,
        js_script_base64: String,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>> {
        create::create(token, name, js_script_base64, runtime_clean_time, env).await
    }

    async fn update(
        &self,
        token: String,
        name: String,
        js_script_base64: String,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>> {
        update::update(token, name, js_script_base64, runtime_clean_time, env).await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        delete::delete(token, name).await
    }

    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        read::read(token, name).await
    }

    async fn run(
        &self,
        token: String,
        js_script_name: String,
        run_type: Option<nodeget_lib::js_runtime::RunType>,
        params: Value,
        env: Option<Value>,
    ) -> RpcResult<Box<RawValue>> {
        run::run(token, js_script_name, run_type, params, env).await
    }

    async fn get_rt_pool(&self, token: String) -> RpcResult<Box<RawValue>> {
        get_rt_pool::get_rt_pool(token).await
    }

    async fn list_all_js_worker(&self, token: String) -> RpcResult<Box<RawValue>> {
        list_all_js_worker::list_all_js_worker(token).await
    }
}
