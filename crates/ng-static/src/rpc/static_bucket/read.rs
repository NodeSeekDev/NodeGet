//! `static-bucket.read` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucket::Read` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_permission;
use crate::ops::read_static;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucket;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket.read` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
///
/// 返回：桶配置模型序列化为 `RawValue`；桶不存在时返回 `NotFound` 错误。
pub async fn read(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket", name = %name, "processing static-bucket_read request");

        check_static_bucket_permission(&token, &name, StaticBucket::Read).await?;
        debug!(target: "static_bucket", name = %name, "static-bucket_read permission check passed");

        let model = read_static(&name)
            .await?
            .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

        serde_json::value::to_raw_value(&model)
            .map_err(|e| NodegetError::SerializationError(format!("Failed to serialize static: {e}")).into())
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
