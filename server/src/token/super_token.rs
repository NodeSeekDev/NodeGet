use crate::DB;
use crate::entity::token;
use crate::token::cache::TokenCache;
use crate::token::hash_string;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::generate_random_string;
use sea_orm::{EntityTrait, Set, TransactionTrait};
use tracing::debug;

async fn insert_new_super_token(
    db: &sea_orm::DatabaseConnection,
) -> anyhow::Result<(String, String)> {
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
        .map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to initialize super token: {e}"))
        })?;

    debug!(target: "token", "Super token inserted into database");
    Ok((full_token, raw_password))
}

// 生成超级令牌，如果已存在则返回 None
//
// # 返回值
// 成功时返回 Some((full_token, raw_password))，如果已存在则返回 None，失败时返回错误消息
pub async fn generate_super_token() -> anyhow::Result<Option<(String, String)>> {
    let db = DB.get().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_string())
    })?;

    // 使用 INSERT OR IGNORE 模式（通过数据库唯一约束）避免 TOCTOU
    // 先尝试插入，如果失败（记录已存在）则返回 None
    match insert_new_super_token(db).await {
        Ok(result) => {
            debug!(target: "token", "Super token generated successfully");
            // Reload cache after creating super token
            if let Err(e) = TokenCache::reload().await {
                tracing::error!(target: "token", error = %e, "Failed to reload token cache after generate_super_token");
            }
            Ok(Some(result))
        }
        Err(e) => {
            // 检查是否是唯一约束冲突（记录已存在）
            let error_msg = format!("{e}");
            if error_msg.contains("UNIQUE constraint failed") || error_msg.contains("duplicate key")
            {
                debug!(target: "token", "Super token already exists, skipping generation");
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

pub async fn roll_super_token() -> anyhow::Result<(String, String)> {
    debug!(target: "token", "Rolling super token - starting transaction");
    let db = DB.get().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_string())
    })?;

    // 使用事务确保删除和插入是原子操作
    // 如果插入失败，删除会回滚，避免锁定
    let result = db
        .transaction::<_, _, sea_orm::DbErr>(|txn| {
            Box::pin(async move {
                // 删除旧令牌
                token::Entity::delete_by_id(1).exec(txn).await?;

                // 生成新令牌数据
                let token_key = generate_random_string(16);
                let token_secret = generate_random_string(32);
                let token_hash = hash_string(&token_secret);
                let username = "root".to_string();
                let raw_password = generate_random_string(32);
                let password_hash = hash_string(&raw_password);

                // 插入新令牌
                let super_token_model = token::ActiveModel {
                    id: Set(1),
                    version: Set(1),
                    token_key: Set(token_key.clone()),
                    token_hash: Set(token_hash),
                    time_stamp_from: Set(None),
                    time_stamp_to: Set(None),
                    token_limit: Set(serde_json::json!([])),
                    username: Set(Some(username)),
                    password_hash: Set(Some(password_hash)),
                };

                token::Entity::insert(super_token_model).exec(txn).await?;

                Ok((format!("{token_key}:{token_secret}"), raw_password))
            })
        })
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("Transaction failed: {e}")))?;

    debug!(target: "token", "Super token rolled successfully");

    // Reload cache after rolling super token
    if let Err(e) = TokenCache::reload().await {
        tracing::error!(target: "token", error = %e, "Failed to reload token cache after roll_super_token");
    }

    Ok(result)
}

// 检查给定的令牌或认证信息是否为超级令牌
//
// # 参数
// * `token_or_auth` - 令牌或认证信息
//
// # 返回值
// 返回布尔值表示是否为超级令牌，失败时返回错误消息
pub async fn check_super_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<bool> {
    let cache = TokenCache::global();
    let super_record = cache.get_super_token().await.ok_or_else(|| {
        NodegetError::NotFound("Super Token record (ID 1) not found in cache".to_owned())
    })?;

    match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            let is_super = key == &super_record.token_key && hash_string(secret) == super_record.token_hash;
            debug!(target: "token", is_super, "Super token check completed (token auth)");
            Ok(is_super)
        }
        TokenOrAuth::Auth(username, password) => {
            let is_super = super_record.username.as_deref() == Some(username.as_str())
                && super_record.password_hash.as_deref() == Some(hash_string(password).as_str());
            debug!(target: "token", is_super, "Super token check completed (basic auth)");
            Ok(is_super)
        }
    }
}
