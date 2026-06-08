//! 内联 JS 调用：从一个 JS Worker 内部调用另一个 Worker。
//!
//! JS 端通过 `globalThis.__nodeget_inline_call_raw(name, paramsJson, timeoutSec, caller)`
//! 触发此模块，将请求转发到 `JsWorkerService::run_inline_call_and_record_result`。
//!
//! 参数和返回值均以 JSON 字符串直传，避免冗余的 parse/serialize 往返。

use crate::js_worker_service::get_js_worker_service;
use crate::server_runtime::js_error;
use crate::spawn_on_server_runtime::spawn_on_server_runtime;
use rquickjs::Error;
use std::result::Result as StdResult;
use tracing::debug;

/// 从 JS 上下文发起内联调用，执行目标 Worker 并返回结果 JSON 字符串。
///
/// - `js_worker_name` —— 目标 Worker 名称
/// - `params_json` —— 调用参数的 JSON 字符串（直接透传，不做 parse）
/// - `timeout_sec` —— 调用方指定的软超时（秒），None 则不限
/// - `inline_caller` —— 发起调用的源 Worker 名称，用于审计
///
/// 内部步骤：
/// 1. 通过 `spawn_on_server_runtime` 在服务器 Runtime 上执行（避免跨 Runtime 资源冲突）
/// 2. 调用 `JsWorkerService::run_inline_call_and_record_result`，直接透传 JSON 字符串
/// 3. 返回结果的 JSON 字符串（不做 serialize）
///
/// # Errors
/// 若内联调用执行失败，返回 `rquickjs::Error`。
pub async fn js_inline_call(
    js_worker_name: String,
    params_json: String,
    timeout_sec: Option<f64>,
    inline_caller: Option<String>,
) -> StdResult<String, Error> {
    debug!(target: "js_runtime", js_worker_name = %js_worker_name, "executing inline call");

    let result_json = spawn_on_server_runtime(async move {
        get_js_worker_service()
            .ok_or_else(|| "JsWorkerService not initialized".to_string())?
            .run_inline_call_and_record_result(
                js_worker_name,
                params_json,
                timeout_sec,
                inline_caller,
            )
            .await
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| js_error("inline_call", e))?
    .map_err(|e| js_error("inline_call", e))?;

    Ok(result_json)
}
