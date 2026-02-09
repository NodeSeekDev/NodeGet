use crate::error::NodegetError;
use anyhow::Result;
use serde_json::value::RawValue;

/// 生成错误消息 JSON 值
///
/// # 参数
/// * `error_id` - 错误 ID
/// * `error_message` - 错误消息文本
///
/// # 返回值
/// 返回包含错误 ID 和错误消息的 JSON 值
pub fn generate_error_message(error_id: impl Into<i128>, error_message: &str) -> serde_json::Value {
    serde_json::json!({
        "error_id": error_id.into(),
        "error_message": error_message
    })
}

/// 将错误代码和消息转换为原始JSON值
///
/// # Errors
///
/// 当JSON序列化失败时返回错误
pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Result<Box<RawValue>> {
    let v = generate_error_message(code, msg);
    serde_json::value::to_raw_value(&v)
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
}

/// 从 `NodegetError` 转换为原始JSON值
///
/// # Errors
///
/// 当JSON序列化失败时返回错误
pub fn nodeget_error_to_raw(error: &NodegetError) -> Result<Box<RawValue>> {
    let json_error = error.to_json_error();
    let v = serde_json::to_value(&json_error)?;
    serde_json::value::to_raw_value(&v)
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
}

/// 将 `anyhow::Error` 转换为原始JSON值
///
/// # Errors
///
/// 当JSON序列化失败时返回错误
pub fn anyhow_error_to_raw(error: &anyhow::Error) -> Result<Box<RawValue>> {
    let nodeget_error = crate::error::anyhow_to_nodeget_error(error);
    nodeget_error_to_raw(&nodeget_error)
}
