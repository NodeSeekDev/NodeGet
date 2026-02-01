use serde_json::value::RawValue;

pub fn generate_error_message(error_id: impl Into<i128>, error_message: &str) -> serde_json::Value {
    serde_json::json!({
        "error_id": error_id.into(),
        "error_message": error_message
    })
}

pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Box<RawValue> {
    let v = generate_error_message(code, msg);
    serde_json::value::to_raw_value(&v).unwrap()
}
