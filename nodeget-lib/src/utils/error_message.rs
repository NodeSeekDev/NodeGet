#[cfg(feature = "for-server")]
pub fn generate_error_message(
    error_id: impl Into<i128>,
    error_message: String,
) -> serde_json::Value {
    serde_json::json!({
        "error_id": error_id.into(),
        "error_message": error_message
    })
}
