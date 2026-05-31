use crate::auth::check_kv_delete_namespace_permission;
use crate::db::delete_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn delete_namespace(token: String, namespace: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, "Processing delete_namespace request");

        // 检查对该命名空间的全局删除权限
        check_kv_delete_namespace_permission(&token, &namespace).await?;
        debug!(target: "kv", namespace = %namespace, "delete_namespace permission check passed");

        delete_kv(namespace).await?;

        debug!(target: "kv", "delete_namespace completed");

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
