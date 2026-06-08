//! `kv_delete_key` RPC 方法：删除指定 namespace 下的单个 key

use crate::auth::check_kv_delete_permission;
use crate::db::delete_key_from_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

/// 删除指定 namespace 下的单个 key
///
/// - `token` — 身份令牌
/// - `namespace` — 命名空间名称
/// - `key` — 要删除的 key
///
/// 返回 `{"success":true}`。
///
/// 内部步骤：
/// 1. 校验 Token 是否拥有该 key 的 `Kv::Delete` 权限
/// 2. 调用 `delete_key_from_kv` 执行数据库删除
/// 3. 返回成功标识
pub async fn delete_key(token: String, namespace: String, key: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, key = %key, "Processing delete_key request");

        // 检查删除权限
        check_kv_delete_permission(&token, &namespace, &key).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "delete_key permission check passed");

        delete_key_from_kv(namespace, key).await?;

        debug!(target: "kv", "delete_key completed");

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
