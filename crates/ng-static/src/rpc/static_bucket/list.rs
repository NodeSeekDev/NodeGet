use super::auth::check_super_token;
use crate::ops::list_all_names;
use ng_core::error::NodegetError;
use serde_json::value::RawValue;
use tracing::{debug, warn};

pub async fn list_rpc(token: String) -> jsonrpsee::core::RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket", "processing static-bucket_list request");

        let is_super_token = check_super_token(&token)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super_token {
            warn!(target: "static_bucket", "non-supertoken attempted to list all static names");
            return Err(NodegetError::PermissionDenied(
                "Only SuperToken can list all static names".to_owned(),
            )
            .into());
        }

        let names = list_all_names().await;
        debug!(target: "static_bucket", count = names.len(), "static-bucket_list completed");

        let json_str = serde_json::to_string(&names).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize static name list: {e}"))
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
