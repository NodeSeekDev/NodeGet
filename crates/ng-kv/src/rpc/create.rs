use crate::auth::check_kv_create_permission;
use crate::db::create_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn create(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %name, "Processing create namespace request");

        // 检查创建命名空间的权限
        check_kv_create_permission(&token).await?;
        debug!(target: "kv", namespace = %name, "Create namespace permission check passed");

        let kv_store = create_kv(name).await?;

        debug!(target: "kv", "Namespace created successfully");

        let json_str = serde_json::to_string(&kv_store).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize KV store: {e}"))
        })?;

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
