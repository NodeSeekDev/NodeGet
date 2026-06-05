//! `static-bucket-file.read` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucketFile::Read` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_file_permission;
use crate::ops::read_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket-file.read` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
/// - `path` - 目标文件相对路径
///
/// 返回：Base64 编码的文件内容序列化为 `RawValue`。
pub async fn read_file_rpc(token: String, name: String, path: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket_file", name = %name, path = %path, "processing static-bucket-file_read request");

        check_static_bucket_file_permission(&token, &name, StaticBucketFile::Read).await?;
        debug!(target: "static_bucket_file", name = %name, "static-bucket-file_read permission check passed");

        let base64_data = read_file(&name, &path).await?;

        serde_json::value::to_raw_value(&base64_data).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize file content: {e}"))
                .into()
        })
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
