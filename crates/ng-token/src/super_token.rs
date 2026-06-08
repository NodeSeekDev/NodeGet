//! 超级令牌（Super Token）的生成、轮换与验证。
//!
//! 超级令牌是 ID 为 1 的特殊 Token，拥有全部权限，不受 Limit 约束。
//! 本模块提供：
//! - `generate_super_token`：首次生成（幂等，已存在时返回 None）
//! - `roll_super_token`：原子性轮换（事务内删除旧记录并插入新记录）
//! - `check_super_token`：验证凭据是否为超级令牌（常量时间比较）

use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::generate_random_string;
use ng_db::entity::token;
use sea_orm::{EntityTrait, Set, TransactionTrait};
use subtle::ConstantTimeEq;
use tracing::debug;

use crate::cache::TokenCache;
use crate::hash_string;
use crate::hash_to_bytes;

/// 向数据库插入一条新的超级令牌记录。
///
/// 固定 ID 为 1，username 为 "root"，token_limit 为空数组（超级令牌不受 Limit 约束）。
///
/// - `db`：数据库连接
/// - 返回：`(full_token, raw_password)` 元组，full_token 格式为 `key:secret`
/// - 错误：数据库插入失败（通常因 ID=1 唯一约束冲突）
async fn insert_new_super_token(
    db: &sea_orm::DatabaseConnection,
) -> anyhow::Result<(String, String)> {
    let token_key = generate_random_string(16);
    let token_secret = generate_random_string(32);
    let full_token = format!("{token_key}:{token_secret}");

    let username = "root".to_string(); // 超级令牌固定用户名
    let raw_password = generate_random_string(32);

    let token_hash = hash_string(&token_secret);
    let password_hash = hash_string(&raw_password);

    let super_token_model = token::ActiveModel {
        id: Set(1), // 超级令牌固定 ID 为 1
        version: Set(1),
        token_key: Set(token_key),
        token_hash: Set(token_hash),
        time_stamp_from: Set(None), // 无时间限制
        time_stamp_to: Set(None),
        token_limit: Set(serde_json::json!([])), // 空 Limit 列表，超级令牌绕过权限检查
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

/// 生成超级令牌，如果已存在则返回 None。
///
/// 使用 INSERT OR IGNORE 模式（通过数据库唯一约束）避免 TOCTOU 竞态：
/// 先尝试插入，若因 ID=1 唯一约束冲突而失败，说明超级令牌已存在。
///
/// - 返回：成功时为 `Some((full_token, raw_password))`，已存在时为 None
/// - 错误：数据库连接未初始化或非约束冲突的数据库错误
pub async fn generate_super_token() -> anyhow::Result<Option<(String, String)>> {
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_string())
    })?;

    match insert_new_super_token(db).await {
        Ok(result) => {
            debug!(target: "token", "Super token generated successfully");
            // 生成成功后刷新缓存
            if let Err(e) = TokenCache::reload().await {
                tracing::error!(target: "token", error = %e, "Failed to reload token cache after generate_super_token");
            }
            Ok(Some(result))
        }
        Err(e) => {
            // 判断是否为唯一约束冲突（SQLite 和 PostgreSQL 的错误消息不同）
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

/// 原子性轮换超级令牌。
///
/// 在事务内先删除旧记录再插入新记录，确保：
/// - 若插入失败，删除会回滚，不会导致超级令牌丢失
/// - 整个操作对外表现为原子的，不会出现中间状态
///
/// - 返回：`(full_token, raw_password)` 元组
/// - 错误：数据库连接未初始化或事务失败
pub async fn roll_super_token() -> anyhow::Result<(String, String)> {
    debug!(target: "token", "Rolling super token - starting transaction");
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_string())
    })?;

    let result = db
        .transaction::<_, _, sea_orm::DbErr>(|txn| {
            Box::pin(async move {
                // 删除旧的超级令牌记录
                token::Entity::delete_by_id(1).exec(txn).await?;

                // 生成新的 key/secret/password
                let token_key = generate_random_string(16);
                let token_secret = generate_random_string(32);
                let token_hash = hash_string(&token_secret);
                let username = "root".to_string();
                let raw_password = generate_random_string(32);
                let password_hash = hash_string(&raw_password);

                // 插入新的超级令牌记录
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

    // 轮换成功后刷新缓存
    if let Err(e) = TokenCache::reload().await {
        tracing::error!(target: "token", error = %e, "Failed to reload token cache after roll_super_token");
    }

    Ok(result)
}

/// 检查给定的凭据是否为超级令牌。
///
/// 从缓存中获取 ID 为 1 的记录，使用常量时间比较（`ct_eq`）验证
/// key/secret 或 username/password 是否匹配。
///
/// - `token_or_auth`：认证凭据
/// - 返回：`true` 表示为超级令牌，`false` 表示不是
/// - 错误：缓存未初始化或缺少超级令牌记录
pub async fn check_super_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<bool> {
    // 检查 TokenCache 是否已初始化，避免未初始化时返回误导性的 NotFound 错误
    if TokenCache::global().is_none() {
        return Err(NodegetError::ConfigNotFound("TokenCache not initialized".to_owned()).into());
    }

    let super_entry = TokenCache::get_super_token().ok_or_else(|| {
        NodegetError::NotFound("Super Token record (ID 1) not found in cache".to_owned())
    })?;

    match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            let key_match: bool = key
                .as_bytes()
                .ct_eq(super_entry.model.token_key.as_bytes())
                .into();
            if !key_match {
                return Ok(false);
            }
            let computed = hash_to_bytes(secret);
            let hash_match: bool = computed.ct_eq(&super_entry.token_hash_bytes).into();
            debug!(target: "token", is_super = hash_match, "Super token check completed (token auth)");
            Ok(hash_match)
        }
        TokenOrAuth::Auth(username, password) => {
            let username_match = super_entry
                .model
                .username
                .as_deref()
                .is_some_and(|u| u.as_bytes().ct_eq(username.as_bytes()).into());
            if !username_match {
                return Ok(false);
            }
            let computed = hash_to_bytes(password);
            let Some(stored) = &super_entry.password_hash_bytes else {
                return Ok(false);
            };
            let hash_match: bool = computed.ct_eq(stored).into();
            debug!(target: "token", is_super = hash_match, "Super token check completed (basic auth)");
            Ok(hash_match)
        }
    }
}
