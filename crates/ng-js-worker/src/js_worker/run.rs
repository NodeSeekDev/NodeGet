use crate::js_worker::auth::check_js_worker_permission;
use crate::service::{enqueue_defined_js_worker_run, enqueue_source_js_worker_run};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::JsWorker as JsWorkerPermission;
use ng_js_runtime::{CompileMode, RunType};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn run(
    token: String,
    js_script_name: String,
    run_type: Option<RunType>,
    params: Value,
    env: Option<Value>,
    compile_mode: Option<CompileMode>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let run_type = run_type.unwrap_or(RunType::Call);
        let compile_mode = compile_mode.unwrap_or(CompileMode::Bytecode);

        let script_name = js_script_name.trim().to_owned();

        if script_name.is_empty() {
            return Err(
                NodegetError::InvalidInput("js_script_name cannot be empty".to_owned()).into(),
            );
        }

        debug!(target: "js_worker", script_name = %script_name, run_type = ?run_type, compile_mode = ?compile_mode, "processing js_worker run request");

        check_js_worker_permission(
            &token,
            script_name.as_str(),
            JsWorkerPermission::RunDefinedJsWorker,
        )
        .await?;

        debug!(target: "js_worker", script_name = %script_name, "js_worker run permission check passed");

        let js_result_id = match compile_mode {
            CompileMode::Bytecode => {
                enqueue_defined_js_worker_run(script_name, run_type, params, env).await?
            }
            CompileMode::Source => {
                enqueue_source_js_worker_run(script_name, run_type, params, env).await?
            }
        };

        debug!(target: "js_worker", js_result_id, "js_worker run enqueued successfully");

        let json_str = serde_json::to_string(&serde_json::json!({ "id": js_result_id }))
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
