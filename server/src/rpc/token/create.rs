use log::debug;
use serde_json::{json, Value};
use nodeget_lib::permission::create::TokenCreationRequest;
use nodeget_lib::utils::error_message::generate_error_message;
use crate::token::generate_token::generate_and_store_token;
use crate::token::split_username_password;

pub async fn create(father_token: String, token_creation: TokenCreationRequest) -> Value {
    let (super_token_arg, super_username_arg, super_password_arg) =
        if let Ok((u, p)) = split_username_password(&father_token) {
            debug!("Token RPC: Detected Username|Password login");
            (None, Some(u.to_string()), Some(p.to_string()))
        } else {
            debug!("Token RPC: Detected Token string login");
            (Some(father_token), None, None)
        };

    let (key, secret) = match generate_and_store_token(
        super_token_arg,
        super_username_arg,
        super_password_arg,
        token_creation.timestamp_from,
        token_creation.timestamp_to,
        token_creation.token_limit,
        token_creation.username,
        token_creation.password,
    )
        .await
    {
        Ok((key, secret)) => (key, secret),
        Err(e) => {
            return generate_error_message(e.0, &e.1);
        }
    };

    json!({
            "key": key,
            "secret": secret,
        })
}