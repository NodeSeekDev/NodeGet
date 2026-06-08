//! `static-bucket.delete` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucket::Delete` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_permission;
use crate::ops::delete_static;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucket;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket.delete` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
///
/// 返回：`{"success":true}` 序列化为 `RawValue`。
pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket", name = %name, "processing static-bucket_delete request");

        check_static_bucket_permission(&token, &name, StaticBucket::Delete).await?;
        debug!(target: "static_bucket", name = %name, "static-bucket_delete permission check passed");

        delete_static(&name).await?;

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
