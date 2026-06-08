//! `ng-js-worker` —— JS Worker 记录管理与 RPC 命名空间。
//!
//! ## 默认 feature（仅类型）
//! - 无独立类型（Worker 类型为 `ng-db` 中的数据库实体）
//!
//! ## `server` feature
//! - `service` 模块 —— 核心 JS Worker 服务（入队执行、内联调用、记录结果）
//! - `js_worker` RPC 命名空间 —— create、read、update、delete、run、get_rt_pool、list_all_js_worker
//! - `js_result` RPC 命名空间 —— query、delete
//! - `auth` 模块 —— 权限校验辅助函数
//! - `rpc_module()` —— 构建并返回合并的 RPC Module（含 `js-worker` 和 `js-result` 两个命名空间）

#![cfg_attr(feature = "server", allow(clippy::too_many_arguments))]

#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
pub mod js_result;
#[cfg(feature = "server")]
pub mod js_worker;
#[cfg(feature = "server")]
pub mod service;

#[cfg(feature = "server")]
pub use service::{
    enqueue_defined_js_worker_run, enqueue_source_js_worker_run, ensure_bytecode_version,
    run_inline_call_and_record_result,
};

#[cfg(feature = "server")]
/// 构建并返回合并的 RPC Module，包含 `js-worker` 和 `js-result` 两个命名空间。
///
/// 在服务器启动时调用此函数以一次性注册两个命名空间。
pub fn rpc_module() -> jsonrpsee::RpcModule<js_worker::JsWorkerRpcImpl> {
    let mut module = js_worker::rpc_module();
    let result_module = js_result::rpc_module();
    module
        .merge(result_module)
        .expect("failed to merge js_result RPC module");
    module
}
