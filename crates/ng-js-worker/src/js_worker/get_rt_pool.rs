use crate::js_worker::auth::check_get_rt_pool_permission;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_js_runtime::runtime_pool;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn get_rt_pool(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "js_worker", "processing get runtime pool request");
        check_get_rt_pool_permission(&token).await?;

        let snapshot = runtime_pool::global_pool().snapshot();

        debug!(target: "js_worker", workers = snapshot.workers.len(), "get_rt_pool completed");
        let json_str = serde_json::to_string(&snapshot)
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
