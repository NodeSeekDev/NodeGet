//! `static-bucket-file.list` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucketFile::List` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_file_permission;
use crate::ops::list_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket-file.list` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
///
/// 返回：文件元数据列表序列化为 `RawValue`。
pub async fn list_file_rpc(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket_file", name = %name, "processing static-bucket-file_list request");

        check_static_bucket_file_permission(&token, &name, StaticBucketFile::List).await?;
        debug!(target: "static_bucket_file", name = %name, "static-bucket-file_list permission check passed");

        let files = list_file(&name).await?;

        serde_json::value::to_raw_value(&files).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize file list: {e}")).into()
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
