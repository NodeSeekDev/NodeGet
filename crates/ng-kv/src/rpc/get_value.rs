use crate::auth::check_kv_read_permission;
use crate::db::get_v_from_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn get_value(token: String, namespace: String, key: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, key = %key, "Processing get_value request");

        // 检查读权限
        check_kv_read_permission(&token, &namespace, &key).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "get_value permission check passed");

        let value = get_v_from_kv(namespace, key).await?;
        let found = value.is_some();

        let json_str = match value {
            Some(v) => serde_json::to_string(&v).map_err(|e| {
                NodegetError::SerializationError(format!("Failed to serialize value: {e}"))
            })?,
            None => "null".to_string(),
        };

        debug!(target: "kv", found, "get_value completed");

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
