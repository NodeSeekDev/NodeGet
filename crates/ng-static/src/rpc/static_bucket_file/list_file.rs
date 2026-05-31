use crate::auth::check_static_bucket_file_permission;
use crate::ops::list_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn list_file_rpc(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket_file", name = %name, "processing static-bucket-file_list request");

        check_static_bucket_file_permission(&token, &name, StaticBucketFile::List).await?;
        debug!(target: "static_bucket_file", name = %name, "static-bucket-file_list permission check passed");

        let files = list_file(&name).await?;

        let json_str = serde_json::to_string(&files).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize file list: {e}"))
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
