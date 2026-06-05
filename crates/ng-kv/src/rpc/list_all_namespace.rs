//! `kv_list_all_namespace` RPC 方法：列出所有 KV 命名空间（按权限过滤）

use crate::auth::{KvNamespaceListPermission, resolve_kv_list_namespace_permission};
use crate::db::list_all_namespaces;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

/// 列出所有 KV 命名空间，按 Token 权限过滤可见范围
///
/// - `token` — 身份令牌
///
/// 返回命名空间名称数组。SuperToken 或 `Scope::Global` 用户可见全部命名空间；
/// 其他用户仅可见其 `Scope::KvNamespace` 中授权的命名空间。
///
/// 内部步骤：
/// 1. 解析 Token 的命名空间列表权限（`All` 或 `Scoped`）
/// 2. 调用 `list_all_namespaces` 查询全部命名空间
/// 3. 按权限过滤后序列化返回
pub async fn list_all_namespace(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", "Processing list_all_namespace request");

        let permission = resolve_kv_list_namespace_permission(&token).await?;
        debug!(target: "kv", "list_all_namespace permission check passed");
        let namespaces = list_all_namespaces().await?;

        let filtered_namespaces = match permission {
            KvNamespaceListPermission::All => namespaces,
            KvNamespaceListPermission::Scoped(allowed) => namespaces
                .into_iter()
                .filter(|namespace| allowed.contains(namespace))
                .collect(),
        };

        debug!(target: "kv", namespace_count = filtered_namespaces.len(), "list_all_namespace completed");

        serde_json::value::to_raw_value(&filtered_namespaces)
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
