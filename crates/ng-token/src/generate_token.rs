//! 子令牌生成与存储。
//!
//! 根据父级（超级）令牌权限生成新的 Token，写入数据库并刷新缓存。
//! 仅超级令牌有权限创建子令牌。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Limit;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::generate_random_string;
use ng_db::entity::token;
use sea_orm::{ActiveValue, EntityTrait, Set};
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::hash_string;
use crate::super_token::check_super_token;

/// 根据父级令牌权限生成并存储新令牌。
///
/// - `father_token_or_auth`：父级令牌或认证信息（必须是超级令牌）
/// - `timestamp_from`：令牌生效时间戳（毫秒），None 表示立即生效
/// - `timestamp_to`：令牌过期时间戳（毫秒），None 表示永不过期
/// - `token_limit`：令牌权限限制列表
/// - `username`：关联用户名，与 password 必须同时提供或同时为 None
/// - `password`：关联密码，与 username 必须同时提供或同时为 None
/// - 返回：成功时为 `(token_key, token_secret)` 元组
/// - 错误：父级令牌非超级令牌、数据库连接未初始化、序列化失败等
///
/// 内部步骤：
/// 1. 验证父级令牌是否为超级令牌
/// 2. 校验 username/password 的成对约束
/// 3. 生成随机 token_key（16 字符）和 token_secret（32 字符）
/// 4. 对 secret 和 password 进行哈希，构建 ActiveModel 插入数据库
/// 5. 刷新 TokenCache
pub async fn generate_and_store_token(
    father_token_or_auth: &TokenOrAuth,

    timestamp_from: Option<i64>,
    timestamp_to: Option<i64>,
    token_limit: Vec<Limit>,

    username: Option<String>,
    password: Option<String>,
) -> anyhow::Result<(String, String)> {
    let is_authorized = check_super_token(father_token_or_auth).await.map_err(|e| {
        warn!(target: "token", "权限拒绝: {e}");
        NodegetError::PermissionDenied(format!("{e}"))
    })?;

    if !is_authorized {
        warn!(target: "token", "权限拒绝: 非超级令牌尝试创建子令牌");
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Only Super Token can create new tokens".to_string(),
        )
        .into());
    }

    debug!(target: "token", "Super token check passed, proceeding with token generation");

    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;

    // username 和 password 必须同时提供或同时为 None，避免创建半配置的认证条目
    if username.is_some() != password.is_some() {
        return Err(NodegetError::ParseError(
            "Username and Password must be both provided or both absent".to_string(),
        )
        .into());
    }

    // username 不能含 `:` / `|`：TokenOrAuth::from_full_token 用这两个字符分隔
    // (冒号优先 Token 模式,管道 Auth 模式)。含分隔符的 username 会导致
    // `username|password` 登录时被误解析为 Token 模式而认证失败(非安全漏洞,
    // 但用户无法正常登录)。fail-fast 在创建时拒绝。
    if let Some(ref username) = username
        && (username.contains(':') || username.contains('|'))
    {
        return Err(NodegetError::InvalidInput(
            "Username cannot contain ':' or '|' characters".to_owned(),
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
        id: ActiveValue::NotSet, // 自增主键，由数据库分配
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

    // 插入成功后刷新缓存，使新令牌立即可用于认证
    if let Err(e) = TokenCache::reload().await {
        tracing::error!(target: "token", error = %e, "Failed to reload token cache after generate_and_store_token");
    }

    Ok((token_key, token_secret))
}
