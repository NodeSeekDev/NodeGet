use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use ng_js_runtime::{CompileMode, RunType};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod create;
mod delete;
mod get_rt_pool;
mod list_all_js_worker;
mod read;
pub mod route_name;
mod run;
mod update;

#[rpc(server, namespace = "js-worker")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(
        &self,
        token: String,
        name: String,
        description: Option<String>,
        js_script_base64: String,
        route_name: Option<String>,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
        max_run_time: Option<i64>,
        max_stack_size: Option<i64>,
        max_heap_size: Option<i64>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "update")]
    async fn update(
        &self,
        token: String,
        name: String,
        description: Option<String>,
        js_script_base64: String,
        route_name: Option<String>,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
        max_run_time: Option<i64>,
        max_stack_size: Option<i64>,
        max_heap_size: Option<i64>,
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
        run_type: Option<RunType>,
        params: Value,
        env: Option<Value>,
        compile_mode: Option<CompileMode>,
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
        description: Option<String>,
        js_script_base64: String,
        route_name: Option<String>,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
        max_run_time: Option<i64>,
        max_stack_size: Option<i64>,
        max_heap_size: Option<i64>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::create", token_key = tk, username = un, name = %name, description = ?description, route_name = ?route_name, runtime_clean_time = ?runtime_clean_time, max_run_time = ?max_run_time, max_stack_size = ?max_stack_size, max_heap_size = ?max_heap_size);
        async {
            rpc_exec!(
                create::create(
                    token,
                    name,
                    description,
                    js_script_base64,
                    route_name,
                    runtime_clean_time,
                    env,
                    max_run_time,
                    max_stack_size,
                    max_heap_size,
                )
                .await
            )
        }
        .instrument(span)
        .await
    }

    async fn update(
        &self,
        token: String,
        name: String,
        description: Option<String>,
        js_script_base64: String,
        route_name: Option<String>,
        runtime_clean_time: Option<i64>,
        env: Option<Value>,
        max_run_time: Option<i64>,
        max_stack_size: Option<i64>,
        max_heap_size: Option<i64>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::update", token_key = tk, username = un, name = %name, description = ?description, route_name = ?route_name, runtime_clean_time = ?runtime_clean_time, max_run_time = ?max_run_time, max_stack_size = ?max_stack_size, max_heap_size = ?max_heap_size);
        async {
            rpc_exec!(
                update::update(
                    token,
                    name,
                    description,
                    js_script_base64,
                    route_name,
                    runtime_clean_time,
                    env,
                    max_run_time,
                    max_stack_size,
                    max_heap_size,
                )
                .await
            )
        }
        .instrument(span)
        .await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::read", token_key = tk, username = un, name = %name);
        async { rpc_exec!(read::read(token, name).await) }
            .instrument(span)
            .await
    }

    async fn run(
        &self,
        token: String,
        js_script_name: String,
        run_type: Option<RunType>,
        params: Value,
        env: Option<Value>,
        compile_mode: Option<CompileMode>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::run", token_key = tk, username = un, js_script_name = %js_script_name, run_type = ?run_type, compile_mode = ?compile_mode);
        async {
            rpc_exec!(run::run(token, js_script_name, run_type, params, env, compile_mode).await)
        }
        .instrument(span)
        .await
    }

    async fn get_rt_pool(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::get_rt_pool", token_key = tk, username = un);
        async { rpc_exec!(get_rt_pool::get_rt_pool(token).await) }
            .instrument(span)
            .await
    }

    async fn list_all_js_worker(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "js_worker", "js-worker::list_all_js_worker", token_key = tk, username = un);
        async { rpc_exec!(list_all_js_worker::list_all_js_worker(token).await) }
            .instrument(span)
            .await
    }
}

/// Build and return an [`jsonrpsee::RpcModule`] with all js-worker RPC methods registered.
pub fn rpc_module() -> jsonrpsee::RpcModule<JsWorkerRpcImpl> {
    JsWorkerRpcImpl.into_rpc()
}
