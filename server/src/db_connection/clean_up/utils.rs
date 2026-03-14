use serde_json::Value;
use uuid::Uuid;

/// 检查字符串是否为有效的 UUID 格式
pub fn is_valid_uuid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok()
}

/// 从 JSON 值中获取指定 key 的毫秒数值
///
/// 支持 JSON number 与 string number 两种格式。
pub fn get_limit_millis(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
}
