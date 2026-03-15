use crate::kv::get_kv_store;
use crate::rpc::kv::auth::check_kv_read_permission_with_pattern;
use crate::rpc::kv::{KvValueItem, NamespaceKeyItem};
use jsonrpsee::core::RpcResult;
use log::debug;
use nodeget_lib::error::NodegetError;
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::HashMap;

fn wildcard_prefix(key_pattern: &str) -> Option<&str> {
    if !key_pattern.contains('*') {
        return None;
    }

    key_pattern.strip_suffix('*')
}

pub async fn get_multi_value(
    token: String,
    namespace_key: Vec<NamespaceKeyItem>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(
            "KV RPC: Processing get_multi_value request, requested count={}",
            namespace_key.len()
        );

        if namespace_key.is_empty() {
            return Err(
                NodegetError::InvalidInput("namespace_key cannot be empty".to_owned()).into(),
            );
        }

        // 先做完整权限校验：任一项无权限则直接拒绝
        for item in &namespace_key {
            if item.namespace.is_empty() {
                return Err(
                    NodegetError::InvalidInput("namespace cannot be empty".to_owned()).into(),
                );
            }
            check_kv_read_permission_with_pattern(&token, &item.namespace, &item.key).await?;
        }

        // 按 namespace 缓存 KVStore，避免重复读取
        let mut namespace_cache = HashMap::new();
        let mut output = Vec::<KvValueItem>::new();

        // 输出顺序与请求顺序保持一致；通配符命中项按 key 字典序输出
        for item in namespace_key {
            let namespace = item.namespace;
            let key_pattern = item.key;

            if !namespace_cache.contains_key(&namespace) {
                let kv_store = get_kv_store(namespace.clone()).await?;
                namespace_cache.insert(namespace.clone(), kv_store);
            }

            let kv_store = namespace_cache
                .get(&namespace)
                .ok_or_else(|| NodegetError::Other("KV namespace cache missing".to_owned()))?;

            if let Some(prefix) = wildcard_prefix(&key_pattern) {
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
            } else {
                let value = kv_store.get(&key_pattern).cloned().unwrap_or(Value::Null);
                output.push(KvValueItem {
                    namespace: namespace.clone(),
                    key: key_pattern,
                    value,
                });
            }
        }

        let json_str = serde_json::to_string(&output).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize kv multi values: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
