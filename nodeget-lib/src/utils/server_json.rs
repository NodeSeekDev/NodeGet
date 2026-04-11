use crate::error::{NodegetError, Result};
use tracing::error;
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value};

/// 将可序列化的值转换为原始JSON值
///
/// # Errors
///
/// 当序列化失败时返回错误
pub fn to_raw_json<T: Serialize>(val: T) -> Result<Box<RawValue>> {
    serde_json::value::to_raw_value(&val).map_err(|e| {
        error!("Serialization error: {e}");
        NodegetError::SerializationError(e.to_string()).into()
    })
}

/// 将可序列化的值转换为原始JSON值，失败时返回错误JSON
///
/// # Errors
///
/// 当序列化失败且回退错误消息也序列化失败时返回错误
pub fn to_raw_json_with_fallback<T: Serialize>(val: T) -> Result<Box<RawValue>> {
    serde_json::value::to_raw_value(&val).or_else(|e| {
        error!("Serialization error: {e}");
        let fallback = serde_json::json!({
            "error_id": 101,
            "error_message": format!("Serialization error: {e}")
        });
        serde_json::value::to_raw_value(&fallback)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    })
}

pub fn try_parse_json_field(map: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(s)) = map.get(key)
        && let Ok(parsed) = serde_json::from_str::<Value>(s)
    {
        map.insert(key.to_string(), parsed);
    }
}

pub fn rename_key(map: &mut Map<String, Value>, old_key: &str, new_key: &str) {
    if let Some(v) = map.remove(old_key) {
        map.insert(new_key.to_string(), v);
    }
}

pub fn rename_and_fix_json(map: &mut Map<String, Value>, old_key: &str, new_key: &str) {
    // 同时完成：取出旧值 -> (如果是 String 则解析) -> 插入新 Key
    if let Some(mut value) = map.remove(old_key) {
        if let Value::String(s) = &value
            && let Ok(parsed) = serde_json::from_str::<Value>(s)
        {
            value = parsed;
        }
        map.insert(new_key.to_string(), value);
    }
}
