use crate::DB;
use crate::entity::token;
use crate::token::hash_string;
use crate::token::super_token::check_super_token;
use nodeget_lib::permission::data_structure::Limit;
use nodeget_lib::utils::generate_random_string;
use sea_orm::{ActiveValue, EntityTrait, Set};
use serde_json;
use nodeget_lib::permission::token_auth::TokenOrAuth;

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
// 成功时返回 (token_key, token_secret) 元组，失败时返回错误代码和消息
pub async fn generate_and_store_token(
    father_token_or_auth: &TokenOrAuth,

    timestamp_from: Option<i64>,
    timestamp_to: Option<i64>,
    token_limit: Vec<Limit>,

    username: Option<String>,
    password: Option<String>,
) -> Result<(String, String), (i64, String)> {
    let is_authorized = check_super_token(father_token_or_auth).await.map_err(|e| (102, e))?;

    if !is_authorized {
        return Err((
            102,
            "Permission Denied: Only Super Token can create new tokens".to_string(),
        ));
    }

    let db = DB
        .get()
        .ok_or_else(|| (103, "Database connection not initialized".to_string()))?;

    if username.is_some() != password.is_some() {
        return Err((
            101,
            "Username and Password must be both provided or both absent".to_string(),
        ));
    }

    let token_key = generate_random_string(16);
    let token_secret = generate_random_string(32);

    let token_hash = hash_string(&token_secret);

    let password_hash_value = password.as_ref().map(|pw| hash_string(pw));

    let token_limit_json = serde_json::to_value(token_limit)
        .map_err(|e| (101, format!("Failed to serialize token limits: {e}")))?;

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
        .map_err(|e| (103, format!("Database insert error: {e}")))?;

    Ok((token_key, token_secret))
}
