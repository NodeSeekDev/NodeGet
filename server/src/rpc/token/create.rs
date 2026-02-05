use crate::token::generate_token::generate_and_store_token;
use log::debug;
use nodeget_lib::permission::create::TokenCreationRequest;
use nodeget_lib::utils::error_message::generate_error_message;
use serde_json::{Value, json};
use nodeget_lib::permission::token_auth::TokenOrAuth;

// 创建新令牌
// 
// # 参数
// * `father_token` - 父级令牌
// * `token_creation` - 令牌创建请求参数
// 
// # 返回值
// 返回创建的令牌信息，包含 key 和 secret
pub async fn create(father_token: String, token_creation: TokenCreationRequest) -> Value {
    let father_token_or_auth = match TokenOrAuth::from_full_token(&father_token) {
        Ok(toa) => {
            toa
        }
        Err(e) => {
            return generate_error_message(101, &format!("Failed to parse token: {e}"))
        }
    };
    
    debug!("Token RPC: Processing token creation request");

    let (key, secret) = match generate_and_store_token(
        &father_token_or_auth,
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
