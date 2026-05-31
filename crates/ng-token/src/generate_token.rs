use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Limit;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::generate_random_string;
use ng_db::entity::token;
use sea_orm::{ActiveValue, EntityTrait, Set};
use tracing::debug;

use crate::cache::TokenCache;
use crate::hash_string;
use crate::super_token::check_super_token;

// 根据父级令牌权限生成并存储新令牌
//
// # 参数
// * `father_token_or_auth` - 父级令牌或认证信息
// * `timestamp_from` - 令牌生效时间戳，可选参数
// * `timestamp_to` - 令牌过期时间戳，可选参数
// * `token_limit` - 令牌权限限制列表
// * `username` - 用户名，可选参数
// * `password` - 密码，可选参数
//
// # 返回值
// 成功时返回 (token_key, token_secret) 元组，失败时返回错误
pub async fn generate_and_store_token(
    father_token_or_auth: &TokenOrAuth,

    timestamp_from: Option<i64>,
    timestamp_to: Option<i64>,
    token_limit: Vec<Limit>,

    username: Option<String>,
    password: Option<String>,
) -> anyhow::Result<(String, String)> {
    let is_authorized = check_super_token(father_token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if !is_authorized {
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Only Super Token can create new tokens".to_string(),
        )
        .into());
    }

    debug!(target: "token", "Super token check passed, proceeding with token generation");

    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;

    if username.is_some() != password.is_some() {
        return Err(NodegetError::ParseError(
            "Username and Password must be both provided or both absent".to_string(),
        )
        .into());
    }

    let has_credentials = username.is_some();
    let token_key = generate_random_string(16);
    let token_secret = generate_random_string(32);
    debug!(target: "token", %token_key, has_credentials, "Token key and secret generated");

    let token_hash = hash_string(&token_secret);

    let password_hash_value = password.as_ref().map(|pw| hash_string(pw));

    let token_limit_json = serde_json::to_value(token_limit).map_err(|e| {
        NodegetError::SerializationError(format!("Failed to serialize token limits: {e}"))
    })?;

    debug!(target: "token", %token_key, "Token limit serialized, building model for DB insert");

    let new_token_model = token::ActiveModel {
        id: ActiveValue::NotSet,
        version: Set(1),
        token_key: Set(token_key.clone()),
        token_hash: Set(token_hash),
        time_stamp_from: Set(timestamp_from),
        time_stamp_to: Set(timestamp_to),
        token_limit: Set(token_limit_json),
        username: Set(username),
        password_hash: Set(password_hash_value),
    };

    token::Entity::insert(new_token_model)
        .exec(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("Database insert error: {e}")))?;

    debug!(target: "token", %token_key, "Token inserted into database successfully");

    // Reload cache after creating new token
    if let Err(e) = TokenCache::reload().await {
        tracing::error!(target: "token", error = %e, "Failed to reload token cache after generate_and_store_token");
    }

    Ok((token_key, token_secret))
}
