//! `JsWorkerService` trait —— JS Worker 服务回调的依赖注入点。
//!
//! 运行时池和 `inline_call/nodeget` 模块需要回调到 js-worker 服务（位于 `ng-js-worker`）。
//! 为打破循环依赖，在此定义 trait 并通过 `OnceLock` 在启动时注入实现，
//! 与 [`ng-core::permission::permission_checker::PermissionChecker`] 使用相同的 `OnceLock` 注入模式。

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

/// JS Worker 服务回调 trait。
///
/// 由 `ng-js-worker`（或 server crate）实现，启动时通过 [`set_js_worker_service`] 注入。
pub trait JsWorkerService: Send + Sync + 'static {
    /// 执行内联调用并记录结果到数据库。
    ///
    /// - `js_script_name` —— 目标 Worker 名称
    /// - `params_json` —— 调用参数的 JSON 字符串（直接透传，避免冗余 parse/serialize）
    /// - `timeout_sec` —— 调用方软超时（秒）
    /// - `inline_caller` —— 发起调用的源 Worker 名称
    ///
    /// 返回执行结果的 JSON 字符串（直接透传，避免冗余 parse/serialize）。
    fn run_inline_call_and_record_result(
        &self,
        js_script_name: String,
        params_json: String,
        timeout_sec: Option<f64>,
        inline_caller: Option<String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>>;

    /// 获取 RPC Module 的克隆，用于分发内部 JSON-RPC 请求。
    fn get_rpc_module(
        &self,
    ) -> Pin<Box<dyn Future<Output = Box<dyn RawJsonDispatcher + Send>> + Send>>;
}

/// 原始 JSON-RPC 分发 trait，对 `jsonrpsee::RpcModule` 的抽象。
///
/// 避免在 `ng-js-runtime` 中直接依赖 `jsonrpsee`。
pub trait RawJsonDispatcher: Send + Sync {
    /// 分发原始 JSON-RPC 请求字符串，返回响应字符串。
    ///
    /// - `json` —— JSON-RPC 请求原始字符串
    /// - `buf_size` —— 响应缓冲区大小
    #[allow(clippy::type_complexity)]
    fn raw_json_request(
        &self,
        json: &str,
        buf_size: usize,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<(String, ())>> + Send + '_>>;
}

static JS_WORKER_SERVICE: OnceLock<Box<dyn JsWorkerService>> = OnceLock::new();

/// 设置全局 `JsWorkerService` 实现。
///
/// 必须在服务器启动时调用一次。
pub fn set_js_worker_service(service: Box<dyn JsWorkerService>) {
    let _ = JS_WORKER_SERVICE.set(service);
}

/// 获取全局 `JsWorkerService`。
///
/// 若未初始化则返回 `None`。
pub fn get_js_worker_service() -> Option<&'static dyn JsWorkerService> {
    JS_WORKER_SERVICE.get().map(std::convert::AsRef::as_ref)
}
