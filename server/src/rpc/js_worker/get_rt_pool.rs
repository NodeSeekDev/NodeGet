use crate::js_runtime::runtime_pool;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use serde_json::value::RawValue;

pub async fn get_rt_pool(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // TODO: token auth
        let _ = token;

        let snapshot = runtime_pool::global_pool().snapshot();
        let json_str = serde_json::to_string(&snapshot)
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
