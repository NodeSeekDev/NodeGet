//! `static-bucket-file.rename` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucketFile::Write` + `StaticBucketFile::Delete` 权限）
//! -> 调用业务层 -> 序列化返回。
//!
//! rename 语义等价于"新建目标 + 删除源"，因此必须同时持有 Write 和 Delete 权限，
//! 避免仅持有 Write 的 Token 通过 rename 绕过 Delete 校验。

use crate::auth::check_static_bucket_file_permission;
use crate::ops::rename_file;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucketFile;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket-file.rename` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
/// - `from` - 源文件相对路径
/// - `to` - 目标文件相对路径
///
/// 返回：`{"success":true}` 序列化为 `RawValue`。
pub async fn rename_file_rpc(
    token: String,
    name: String,
    from: String,
    to: String,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket_file", name = %name, from = %from, to = %to, "processing static-bucket-file_rename request");

        // rename 语义同时等价于"新建 to" + "删除 from"，因此必须同时具备
        // Write 和 Delete 权限，避免仅持有 Write 的 token 绕过 Delete
        check_static_bucket_file_permission(&token, &name, StaticBucketFile::Write).await?;
        check_static_bucket_file_permission(&token, &name, StaticBucketFile::Delete).await?;
        debug!(target: "static_bucket_file", name = %name, "static-bucket-file_rename permission check passed");

        rename_file(&name, &from, &to).await?;

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
