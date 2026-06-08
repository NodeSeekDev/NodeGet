//! `static-bucket-file.upload` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucketFile::Write` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_file_permission;
use crate::ops::upload_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket-file.upload` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
/// - `path` - 文件相对路径
/// - `body` - 二进制文件内容（与 `base64` 二选一）
/// - `base64` - Base64 编码的文件内容（与 `body` 二选一）
///
/// 返回：`{"success":true}` 序列化为 `RawValue`。
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

        serde_json::value::to_raw_value(&serde_json::json!({"success": true}))
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
