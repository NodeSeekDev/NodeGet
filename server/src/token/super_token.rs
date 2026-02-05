use crate::DB;
use crate::entity::token;
use crate::token::hash_string;
use nodeget_lib::utils::generate_random_string;
use sea_orm::{EntityTrait, Set};

// 生成超级令牌，如果已存在则返回 None
// 
// # 返回值
// 成功时返回 Some((full_token, raw_password))，如果已存在则返回 None，失败时返回错误消息
pub async fn generate_super_token() -> Result<Option<(String, String)>, String> {
    let db = DB
        .get()
        .ok_or_else(|| "Database connection not initialized".to_string())?;

    let existing_super = token::Entity::find_by_id(1)
        .one(db)
        .await
        .map_err(|e| format!("Database query error: {e}"))?;

    if existing_super.is_some() {
        return Ok(None);
    }

    let token_key = generate_random_string(16);
    let token_secret = generate_random_string(32);
    let full_token = format!("{token_key}:{token_secret}");

    let username = "root".to_string();
    let raw_password = generate_random_string(32);

    let token_hash = hash_string(&token_secret);

    let password_hash = hash_string(&raw_password);

    let super_token_model = token::ActiveModel {
        id: Set(1),
        version: Set(1),
        token_key: Set(token_key),
        token_hash: Set(token_hash),
        time_stamp_from: Set(None),
        time_stamp_to: Set(None),
        token_limit: Set(serde_json::json!([])),
        username: Set(Some(username)),
        password_hash: Set(Some(password_hash)),
    };

    token::Entity::insert(super_token_model)
        .exec(db)
        .await
        .map_err(|e| format!("Failed to initialize super token: {e}"))?;

    Ok(Some((full_token, raw_password)))
}

use nodeget_lib::permission::token_auth::TokenOrAuth;

// 检查给定的令牌或认证信息是否为超级令牌
// 
// # 参数
// * `token_or_auth` - 令牌或认证信息
// 
// # 返回值
// 返回布尔值表示是否为超级令牌，失败时返回错误消息
pub async fn check_super_token(
    token_or_auth: &TokenOrAuth,
) -> Result<bool, String> {
    let db = DB.get().ok_or("Database connection not initialized")?;
    let super_record = token::Entity::find_by_id(1)
        .one(db)
        .await
        .map_err(|e| format!("Database error: {e}"))?
        .ok_or("Super Token record (ID 1) not found in database")?;

    match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            if key == &super_record.token_key {
                let hash = hash_string(secret);
                Ok(hash == super_record.token_hash)
            } else {
                Ok(false)
            }
        },
        TokenOrAuth::Auth(username, password) => {
            if Some(username.clone()) == super_record.username {
                let hash = hash_string(password);
                Ok(Some(hash) == super_record.password_hash)
            } else {
                Ok(false)
            }
        }
    }
}