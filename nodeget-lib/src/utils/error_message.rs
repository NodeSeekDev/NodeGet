use serde_json::value::RawValue;

// 生成错误消息 JSON 值
//
// # 参数
// * `error_id` - 错误 ID
// * `error_message` - 错误消息文本
//
// # 返回值
// 返回包含错误 ID 和错误消息的 JSON 值
pub fn generate_error_message(error_id: impl Into<i128>, error_message: &str) -> serde_json::Value {
    serde_json::json!({
        "error_id": error_id.into(),
        "error_message": error_message
    })
}

/// 将错误代码和消息转换为原始JSON值
///
/// # Panics
///
/// 当JSON序列化失败时会发生panic（理论上不应发生，因为错误消息是简单的字符串和数字）
pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Box<RawValue> {
    let v = generate_error_message(code, msg);
    serde_json::value::to_raw_value(&v).unwrap()
}
