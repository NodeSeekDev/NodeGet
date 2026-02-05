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

// 将错误信息转换为 RawValue 格式
//
// # 参数
// * `code` - 错误代码
// * `msg` - 错误消息文本
//
// # 返回值
// 返回序列化后的 RawValue 格式错误信息
pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Box<RawValue> {
    let v = generate_error_message(code, msg);
    serde_json::value::to_raw_value(&v).unwrap()
}
