use crate::auth::check_kv_write_permission;
use crate::db::set_v_to_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn set_value(
    token: String,
    namespace: String,
    key: String,
    value: Value,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, key = %key, "Processing set_value request");

        // 检查写权限
        check_kv_write_permission(&token, &namespace, &key).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "set_value permission check passed");

        set_v_to_kv(namespace, key, value).await?;

        debug!(target: "kv", "set_value completed");

        let json_str = "{\"success\":true}".to_string();

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
