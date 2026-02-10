use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// KV 存储结构体
///
/// 每个 KVStore 代表一个命名空间，包含一个 HashMap 存储键值对
/// 其中 key 是字符串，value 是任意 JSON 值
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct KVStore {
    /// 命名空间名称，作为唯一标识符
    namespace: String,
    /// 存储键值对的 HashMap
    kv: HashMap<String, serde_json::Value>,
}

impl KVStore {
    /// 创建一个新的 KVStore
    ///
    /// # 参数
    /// * `namespace` - 命名空间名称
    ///
    /// # 返回值
    /// 返回一个新的 KVStore 实例
    pub fn new(namespace: String) -> Self {
        Self {
            namespace,
            kv: HashMap::new(),
        }
    }

    /// 获取命名空间名称
    ///
    /// # 返回值
    /// 返回命名空间名称的引用
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// 获取指定 key 的值
    ///
    /// # 参数
    /// * `key` - 键名
    ///
    /// # 返回值
    /// 如果 key 存在，返回 Some(&Value)，否则返回 None
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.kv.get(key)
    }

    /// 设置指定 key 的值
    ///
    /// # 参数
    /// * `key` - 键名
    /// * `value` - 值（任意 JSON 类型）
    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.kv.insert(key, value);
    }

    /// 删除指定 key
    ///
    /// # 参数
    /// * `key` - 键名
    ///
    /// # 返回值
    /// 如果 key 存在并被删除，返回 Some(Value)，否则返回 None
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.kv.remove(key)
    }

    /// 检查是否包含指定 key
    ///
    /// # 参数
    /// * `key` - 键名
    ///
    /// # 返回值
    /// 如果 key 存在返回 true，否则返回 false
    pub fn contains_key(&self, key: &str) -> bool {
        self.kv.contains_key(key)
    }

    /// 获取所有的 key
    ///
    /// # 返回值
    /// 返回所有 key 的列表
    pub fn keys(&self) -> Vec<String> {
        self.kv.keys().cloned().collect()
    }

    /// 获取所有的 value
    ///
    /// # 返回值
    /// 返回所有 value 的列表
    pub fn values(&self) -> Vec<&serde_json::Value> {
        self.kv.values().collect()
    }

    /// 获取键值对的数量
    ///
    /// # 返回值
    /// 返回键值对的数量
    pub fn len(&self) -> usize {
        self.kv.len()
    }

    /// 检查是否为空
    ///
    /// # 返回值
    /// 如果没有键值对返回 true，否则返回 false
    pub fn is_empty(&self) -> bool {
        self.kv.is_empty()
    }

    /// 清空所有键值对
    pub fn clear(&mut self) {
        self.kv.clear();
    }

    /// 获取内部的 HashMap 引用
    ///
    /// # 返回值
    /// 返回内部 HashMap 的引用
    pub fn inner(&self) -> &HashMap<String, serde_json::Value> {
        &self.kv
    }

    /// 获取内部的 HashMap 可变引用
    ///
    /// # 返回值
    /// 返回内部 HashMap 的可变引用
    pub fn inner_mut(&mut self) -> &mut HashMap<String, serde_json::Value> {
        &mut self.kv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kv_store_basic_operations() {
        let mut store = KVStore::new("test".to_string());

        // Test set and get
        store.set("key1".to_string(), serde_json::json!("value1"));
        assert_eq!(store.get("key1"), Some(&serde_json::json!("value1")));

        // Test contains_key
        assert!(store.contains_key("key1"));
        assert!(!store.contains_key("key2"));

        // Test remove
        let removed = store.remove("key1");
        assert_eq!(removed, Some(serde_json::json!("value1")));
        assert!(!store.contains_key("key1"));

        // Test len and is_empty
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        // Add more items
        store.set("key2".to_string(), serde_json::json!(42));
        store.set("key3".to_string(), serde_json::json!({"nested": "object"}));
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());

        // Test keys
        let keys = store.keys();
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));

        // Test clear
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_kv_store_namespace() {
        let store = KVStore::new("my_namespace".to_string());
        assert_eq!(store.namespace(), "my_namespace");
    }
}
