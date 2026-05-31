use crate::auth::check_static_bucket_file_permission;
use crate::ops::upload_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn upload_file_rpc(
    token: String,
    name: String,
    path: String,
    body: Option<Vec<u8>>,
    base64: Option<String>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket_file", name = %name, path = %path, "processing static-bucket-file_upload request");

        check_static_bucket_file_permission(&token, &name, StaticBucketFile::Write).await?;
        debug!(target: "static_bucket_file", name = %name, "static-bucket-file_upload permission check passed");

        upload_file(&name, &path, body, base64).await?;

        let json_str = r#"{"success":true}"#.to_string();

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
