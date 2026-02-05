use crate::token::get::get_token;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::generate_error_message;
use serde_json::Value;

// 获取令牌信息
//
// # 参数
// * `token` - 认证令牌
//
// # 返回值
// 返回令牌信息的 JSON 值
pub async fn get(token: String) -> Value {
    let token_or_auth = match TokenOrAuth::from_full_token(&token) {
        Ok(toa) => toa,
        Err(e) => return generate_error_message(101, &format!("Failed to parse token: {e}")),
    };

    match get_token(&token_or_auth).await {
        Ok(token_info) => serde_json::to_value(token_info).unwrap_or_else(|e| {
            generate_error_message(101, &format!("Failed to serialize token info: {e}"))
        }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}
