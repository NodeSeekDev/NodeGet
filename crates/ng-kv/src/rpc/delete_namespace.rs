//! `kv_delete_namespace` RPC 方法：删除整个 KV 命名空间

use crate::auth::check_kv_delete_namespace_permission;
use crate::db::delete_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

/// 删除整个命名空间及其所有 key
///
/// - `token` — 身份令牌，需拥有该命名空间的 `Kv::Delete("*")` 全局删除权限
/// - `namespace` — 要删除的命名空间名称
///
/// 返回 `{"success":true}`。
///
/// 内部步骤：
/// 1. 校验 Token 是否拥有该命名空间的全局删除权限
/// 2. 调用 `delete_kv` 删除该命名空间所有记录
/// 3. 返回成功标识
pub async fn delete_namespace(token: String, namespace: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, "Processing delete_namespace request");

        // 检查对该命名空间的全局删除权限
        check_kv_delete_namespace_permission(&token, &namespace).await?;
        debug!(target: "kv", namespace = %namespace, "delete_namespace permission check passed");

        delete_kv(namespace).await?;

        debug!(target: "kv", "delete_namespace completed");

        serde_json::value::to_raw_value(&serde_json::json!({"success": true}))
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
