use log::debug;
use serde_json::Value;
use nodeget_lib::utils::error_message::generate_error_message;
use crate::token::get::get_token;
use crate::token::{parse_token_and_auth, split_username_password};

pub async fn get(token: String) -> Value {
    let (token_arg, username_arg, password_arg) = parse_token_and_auth(&token);

    match get_token(token_arg, username_arg, password_arg).await {
        Ok(token_info) => serde_json::to_value(token_info).unwrap_or_else(|e| {
            generate_error_message(101, &format!("Failed to serialize token info: {e}"))
        }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}