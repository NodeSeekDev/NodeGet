use crate::error::{NodegetError, anyhow_to_nodeget_error};
use anyhow::Result;
use serde_json::value::RawValue;

pub fn generate_error_message(error_id: impl Into<i128>, error_message: &str) -> serde_json::Value {
    serde_json::json!({
        "error_id": error_id.into(),
        "error_message": error_message
    })
}

pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Result<Box<RawValue>> {
    let v = generate_error_message(code, msg);
    serde_json::value::to_raw_value(&v)
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
}

pub fn nodeget_error_to_raw(error: &NodegetError) -> Result<Box<RawValue>> {
    let json_error = error.to_json_error();
    let v = serde_json::to_value(&json_error)?;
    serde_json::value::to_raw_value(&v)
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
}

pub fn anyhow_error_to_raw(error: &anyhow::Error) -> Result<Box<RawValue>> {
    let nodeget_error = anyhow_to_nodeget_error(error);
    nodeget_error_to_raw(&nodeget_error)
}
