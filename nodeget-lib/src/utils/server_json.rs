use log::error;
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value};

// 直接序列化为 RawValue，避免 Value 树，仅在启用 for-server 特性时编译
//
// # 参数
// * `val` - 需要序列化的值
//
// # 返回值
// 返回序列化后的 RawValue
pub fn to_raw_json<T: Serialize>(val: T) -> Box<RawValue> {
    serde_json::value::to_raw_value(&val).unwrap_or_else(|e| {
        error!("Serialization error: {e}");
        // fallback
        serde_json::value::to_raw_value(&serde_json::json!({
            "error_id": 101,
            "error_message": format!("Serialization error: {e}")
        }))
        .unwrap()
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
