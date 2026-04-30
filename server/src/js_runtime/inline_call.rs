use crate::js_runtime::js_error;
use crate::rpc::js_worker::service::run_inline_call_and_record_result;
use rquickjs::Error;
use serde_json::Value;
use std::result::Result as StdResult;
use tracing::debug;

/// # Errors
/// Returns an error if the params are not valid JSON or the inline call fails.
pub async fn js_inline_call(
    js_worker_name: String,
    params_json: String,
    timeout_sec: Option<f64>,
    inline_caller: Option<String>,
) -> StdResult<String, Error> {
    debug!(target: "js_runtime", js_worker_name = %js_worker_name, "executing inline call");
    let params: Value = serde_json::from_str(&params_json).map_err(|e| {
        js_error(
            "inline_call",
            format!("inline_call params is not valid JSON: {e}"),
        )
    })?;

    let result_value =
        run_inline_call_and_record_result(js_worker_name, params, timeout_sec, inline_caller)
            .await
            .map_err(|e| js_error("inline_call", e.to_string()))?;

    serde_json::to_string(&result_value).map_err(|e| {
        js_error(
            "inline_call",
            format!("Failed to serialize inline_call result: {e}"),
        )
    })
}
