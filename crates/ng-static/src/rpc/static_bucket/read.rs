use crate::auth::check_static_bucket_permission;
use crate::ops::read_static;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucket;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn read(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket", name = %name, "processing static-bucket_read request");

        check_static_bucket_permission(&token, &name, StaticBucket::Read).await?;
        debug!(target: "static_bucket", name = %name, "static-bucket_read permission check passed");

        let model = read_static(&name)
            .await?
            .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

        let json_str = serde_json::to_string(&model).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize static: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
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
