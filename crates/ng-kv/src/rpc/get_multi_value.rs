//! `kv_get_multi_value` RPC 方法：批量读取多个 namespace/key 的值，支持后缀通配符

use crate::auth::check_kv_read_permission_with_pattern;
use crate::db::{get_kv_store_optional, get_v_from_kv_lenient};
use crate::rpc::KvValueItem;
use crate::rpc::NamespaceKeyItem;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::HashMap;
use tracing::{debug, warn};

/// 从通配符 key pattern 中提取前缀部分
///
/// - `metadata_*` → `Some("metadata_")`
/// - `abc` → `None`（非通配符）
/// - `*` → `Some("")`（匹配所有 key）
fn wildcard_prefix(key_pattern: &str) -> Option<&str> {
    if !key_pattern.contains('*') {
        return None;
    }

    key_pattern.strip_suffix('*')
}

/// 批量读取多个 namespace/key 的值
///
/// - `token` — 身份令牌，需拥有每个 namespace/key 的读权限
/// - `namespace_key` — 待查询的 namespace+key 列表，key 支持后缀通配符（如 `metadata_*`）
///
/// 返回 `Vec<KvValueItem>`，精确 key 不存在时 value 为 null。
/// 输出顺序与请求顺序一致，通配符命中的项按 key 字典序排列。
///
/// 内部步骤：
/// 1. 逐项校验读权限，任一项无权限则整体拒绝
/// 2. 按 namespace 缓存 KVStore，避免同一命名空间重复读取
/// 3. 处理通配符：提取前缀后在 KVStore 中过滤匹配 key
/// 4. 序列化结果为 RawValue 返回
pub async fn get_multi_value(
    token: String,
    namespace_key: Vec<NamespaceKeyItem>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(
            target: "kv",
            count = namespace_key.len(),
            "Processing get_multi_value request"
        );

        if namespace_key.is_empty() {
            warn!(target: "kv", "验证失败: namespace_key 为空");
            return Err(
                NodegetError::InvalidInput("namespace_key cannot be empty".to_owned()).into(),
            );
        }

        // 先做完整权限校验：任一项无权限则直接拒绝
        for item in &namespace_key {
            if item.namespace.is_empty() {
                warn!(target: "kv", "验证失败: namespace 为空");
                return Err(
                    NodegetError::InvalidInput("namespace cannot be empty".to_owned()).into(),
                );
            }
            check_kv_read_permission_with_pattern(&token, &item.namespace, &item.key).await?;
        }
        debug!(target: "kv", items_count = namespace_key.len(), "get_multi_value permission checks passed");

        // 按 namespace 缓存 KVStore（仅通配符请求会填充），避免重复读取
        let mut namespace_cache: HashMap<String, crate::KVStore> = HashMap::new();
        let mut output = Vec::<KvValueItem>::new();

        // 输出顺序与请求顺序保持一致；通配符命中项按 key 字典序输出
        for item in namespace_key {
            let namespace = item.namespace;
            let key_pattern = item.key;

            // 精确 key 快速路径：若该 namespace 未被通配符请求加载过缓存，
            // 直接单行查询，跳过加载整个 namespace（避免 10000 key 的 namespace
            // 仅为读 1 个精确 key 而全量加载）。若 namespace 已在缓存中（前面的
            // 通配符请求加载），则复用缓存避免重复查询。
            if wildcard_prefix(&key_pattern).is_none() {
                let value = if let Some(store) = namespace_cache.get(&namespace) {
                    store.get(&key_pattern).cloned().unwrap_or(Value::Null)
                } else {
                    get_v_from_kv_lenient(&namespace, &key_pattern)
                        .await?
                        .unwrap_or(Value::Null)
                };
                output.push(KvValueItem {
                    namespace,
                    key: key_pattern,
                    value,
                });
                continue;
            }

            // 通配符路径：必须加载整个 namespace 才能按前缀过滤
            if !namespace_cache.contains_key(&namespace)
                && let Some(kv_store) = get_kv_store_optional(namespace.clone()).await?
            {
                namespace_cache.insert(namespace.clone(), kv_store);
            }

            let kv_store = if let Some(store) = namespace_cache.get(&namespace) {
                store
            } else {
                // namespace 不存在：通配符跳过（无命中）
                continue;
            };

            // 通配符分支（wildcard_prefix 必为 Some）
            let prefix = wildcard_prefix(&key_pattern).expect("checked wildcard above");
            let mut matched_keys: Vec<&str> = kv_store
                .inner()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .map(String::as_str)
                .collect();
            matched_keys.sort_unstable();

            for key in matched_keys {
                if let Some(value) = kv_store.get(key) {
                    output.push(KvValueItem {
                        namespace: namespace.clone(),
                        key: key.to_owned(),
                        value: value.clone(),
                    });
                }
            }
        }

        debug!(target: "kv", result_count = output.len(), "get_multi_value completed");

        serde_json::value::to_raw_value(&output)
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
