//! `kv_get_value` RPC 方法：读取指定 namespace 下单个 key 的值

use crate::auth::check_kv_read_permission;
use crate::db::get_v_from_kv;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

/// 读取指定 namespace 下单个 key 的值
///
/// - `token` — 身份令牌，需拥有该 namespace/key 的 `Kv::Read` 权限
/// - `namespace` — 命名空间名称
/// - `key` — 要读取的 key
///
/// 返回 key 对应的 JSON 值，key 不存在时返回 `null`。
///
/// 内部步骤：
/// 1. 校验 Token 是否拥有该 key 的读权限
/// 2. 调用 `get_v_from_kv` 从数据库查询
/// 3. key 存在则序列化值，不存在则返回 `null`
pub async fn get_value(token: String, namespace: String, key: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", namespace = %namespace, key = %key, "Processing get_value request");

        // 检查读权限
        check_kv_read_permission(&token, &namespace, &key).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "get_value permission check passed");

        let value = get_v_from_kv(namespace, key).await?;
        let found = value.is_some();

        debug!(target: "kv", found, "get_value completed");

        match value {
            Some(v) => serde_json::value::to_raw_value(&v)
                .map_err(|e| NodegetError::SerializationError(format!("{e}")).into()),
            None => RawValue::from_string("null".to_owned())
                .map_err(|e| NodegetError::SerializationError(format!("{e}")).into()),
        }
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
