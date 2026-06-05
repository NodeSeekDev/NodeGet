//! `static-bucket.update` RPC 实现。
//!
//! 职责：鉴权（需 `StaticBucket::Write` 权限） -> 调用业务层 -> 序列化返回。

use crate::auth::check_static_bucket_permission;
use crate::ops::update_static;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::StaticBucket;
use serde_json::value::RawValue;
use tracing::debug;

/// 处理 `static-bucket.update` RPC 请求。
///
/// - `token` - 认证 Token
/// - `name` - 目标桶名称
/// - `path` - 新的磁盘子目录路径
/// - `is_http_root` - 是否设为 HTTP 根路径回退桶
/// - `cors` - 是否启用 CORS
/// - `enable` - 是否启用（`None` 表示不修改）
///
/// 返回：更新后的桶配置模型序列化为 `RawValue`。
pub async fn update(
    token: String,
    name: String,
    path: String,
    is_http_root: bool,
    cors: bool,
    enable: Option<bool>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "static_bucket", name = %name, "processing static-bucket_update request");

        check_static_bucket_permission(&token, &name, StaticBucket::Write).await?;
        debug!(target: "static_bucket", name = %name, "static-bucket_update permission check passed");

        let model = update_static(name, path, is_http_root, cors, enable).await?;

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
