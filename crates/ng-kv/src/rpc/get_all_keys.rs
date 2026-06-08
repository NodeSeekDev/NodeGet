//! `kv_get_all_keys` RPC 方法：列出指定命名空间下所有 key

use crate::auth::check_kv_list_keys_permission;
use crate::db::get_keys_from_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

/// 列出指定命名空间下所有 key
///
/// - `token` — 身份令牌，需拥有该命名空间的 `Kv::ListAllKeys` 权限
/// - `namespace` — 命名空间名称
///
/// 返回 key 字符串数组，按字典序排列。
///
/// 内部步骤：
/// 1. 校验 Token 是否拥有 `ListAllKeys` 权限
/// 2. 调用 `get_keys_from_kv` 查询所有 key
/// 3. 序列化结果为 RawValue 返回
pub async fn get_all_keys(token: String, namespace: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, "Processing get_all_keys request");

        // 检查列出 keys 的权限
        check_kv_list_keys_permission(&token, &namespace).await?;
        debug!(target: "kv", namespace = %namespace, "get_all_keys permission check passed");

        let keys = get_keys_from_kv(namespace).await?;

        debug!(target: "kv", keys_count = keys.len(), "get_all_keys completed");

        serde_json::value::to_raw_value(&keys)
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
