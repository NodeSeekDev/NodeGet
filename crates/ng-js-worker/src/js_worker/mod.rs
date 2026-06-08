//! `js-worker` RPC 命名空间 —— JS Worker 的 CRUD、执行和运行时池管理。
//!
//! 提供七个 RPC 方法：
//! - `js-worker_create` —— 创建新 Worker（编译字节码并入库）
//! - `js-worker_update` —— 更新 Worker（重新编译字节码、驱逐旧 Runtime）
//! - `js-worker_delete` —— 删除 Worker（驱逐 Runtime 后从数据库移除）
//! - `js-worker_read` —— 读取 Worker 详情
//! - `js-worker_run` —— 执行 Worker（字节码或源码模式）
//! - `js-worker_get_rt_pool` —— 获取运行时池状态快照
//! - `js-worker_list_all_js_worker` —— 列出所有可见 Worker 名称

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

/// `js-worker` RPC trait 定义。
#[rpc(server, namespace = "js-worker")]
#[allow(clippy::too_many_arguments)]
pub trait Rpc {
    /// 创建新的 JS Worker（编译字节码并入库）。
    #[allow(clippy::too_many_arguments)]
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

    /// 更新 JS Worker（重新编译字节码、驱逐旧 Runtime）。
    #[allow(clippy::too_many_arguments)]
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

    /// 删除 JS Worker（驱逐 Runtime 后从数据库移除）。
    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 读取指定 JS Worker 的详情。
    #[method(name = "read")]
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 执行 JS Worker（支持字节码或源码模式）。
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

    /// 获取运行时池状态快照。
    #[method(name = "get_rt_pool")]
    async fn get_rt_pool(&self, token: String) -> RpcResult<Box<RawValue>>;

    /// 列出所有可见的 JS Worker 名称。
    #[method(name = "list_all_js_worker")]
    async fn list_all_js_worker(&self, token: String) -> RpcResult<Box<RawValue>>;
}

/// `js-worker` RPC 实现。
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

/// 构建并返回 `js-worker` 命名空间的 RPC Module。
pub fn rpc_module() -> jsonrpsee::RpcModule<JsWorkerRpcImpl> {
    JsWorkerRpcImpl.into_rpc()
}
