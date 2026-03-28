use crate::rpc::js_worker::auth::check_js_worker_permission;
use crate::rpc::js_worker::service::enqueue_defined_js_worker_run;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::js_runtime::RunType;
use nodeget_lib::permission::data_structure::JsWorker as JsWorkerPermission;
use serde_json::Value;
use serde_json::value::RawValue;

pub async fn run(
    token: String,
    js_script_name: String,
    run_type: Option<RunType>,
    params: Value,
    env: Option<Value>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let run_type = run_type.unwrap_or(RunType::Call);

        let script_name = js_script_name.trim().to_owned();

        if script_name.is_empty() {
            return Err(NodegetError::InvalidInput("js_script_name cannot be empty".to_owned()).into());
        }

        check_js_worker_permission(
            &token,
            script_name.as_str(),
            JsWorkerPermission::RunDefinedJsWorker,
        )
        .await?;

        let js_result_id =
            enqueue_defined_js_worker_run(script_name, run_type, params, env).await?;

        let json_str = serde_json::to_string(&serde_json::json!({ "id": js_result_id }))
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
