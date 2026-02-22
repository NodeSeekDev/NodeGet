use crate::DB;
use crate::entity::token;
use crate::token::hash_string;
use crate::token::super_token::check_super_token;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Limit, Permission, Scope, Token};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

// 根据令牌或认证信息获取令牌详细信息
//
// # 参数
// * `token_or_auth` - 令牌或认证信息
//
// # 返回值
// 成功时返回令牌信息，失败时返回错误
pub async fn get_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<Token> {
    let db = DB.get().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;

    // 验证认证信息并从数据库获取对应的token记录
    let token_model = match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            // TokenKey:TokenSecret 认证方式
            let model = token::Entity::find()
                .filter(token::Column::TokenKey.eq(key))
                .one(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(format!("Database query error: {e}")))?
                .ok_or_else(|| {
                    NodegetError::NotFound("Token key not found in database".to_owned())
                })?;

            if model.token_hash != hash_string(secret) {
                return Err(
                    NodegetError::PermissionDenied("Invalid token secret".to_owned()).into(),
                );
            }

            model
        }
        TokenOrAuth::Auth(username, password) => {
            // Username|Password 认证方式
            let model = token::Entity::find()
                .filter(token::Column::Username.eq(username))
                .one(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(format!("Database query error: {e}")))?
                .ok_or_else(|| {
                    NodegetError::NotFound("Username not found in database".to_owned())
                })?;

            let p_hash = hash_string(password);
            if model.password_hash != Some(p_hash) {
                return Err(NodegetError::PermissionDenied("Invalid password".to_owned()).into());
            }

            model
        }
    };

    let token_limit: Vec<Limit> = serde_json::from_value(token_model.token_limit).map_err(|e| {
        NodegetError::SerializationError(format!("Failed to parse token permissions: {e}"))
    })?;

    Ok(Token {
        version: token_model.version,
        token_key: token_model.token_key,
        timestamp_from: token_model.time_stamp_from,
        timestamp_to: token_model.time_stamp_to,
        token_limit,
        username: token_model.username,
    })
}

// 检查令牌是否有足够的权限执行特定操作
//
// # 参数
// * `token_or_auth` - 令牌或认证信息
// * `scopes` - 请求的操作范围列表
// * `permissions` - 请求的权限列表
//
// # 返回值
// 返回布尔值表示是否有足够权限，失败时返回错误
pub async fn check_token_limit(
    token_or_auth: &TokenOrAuth,
    scopes: Vec<Scope>,
    permissions: Vec<Permission>,
) -> anyhow::Result<bool> {
    // 检查超级Token权限
    let is_super_token = check_super_token(token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if is_super_token {
        return Ok(true);
    }

    // 获取并验证Token
    let token = get_token(token_or_auth).await?;

    // 检查Token有效期
    let now = get_local_timestamp_ms_i64()?;

    if let Some(from) = token.timestamp_from
        && now < from
    {
        return Ok(false);
    }

    if let Some(to) = token.timestamp_to
        && now > to
    {
        return Ok(false);
    }

    // 检查权限范围
    // 对于传入的每一个 Scope 和每一个 Permission，Token 中必须至少有一个 Limit 规则能够同时满足它们。
    // 即：请求的 (Scope, Permission) 必须被 Token 的 Limit 集合覆盖。
    for req_scope in &scopes {
        for req_perm in &permissions {
            let mut is_allowed = false;

            for limit in &token.token_limit {
                let scope_covered = {
                    limit
                        .scopes
                        .iter()
                        .any(|limit_scope| match (limit_scope, req_scope) {
                            (Scope::Global, _) => true, // 全局权限可以操作任何具体 Scope
                            (Scope::AgentUuid(limit_id), Scope::AgentUuid(req_id)) => {
                                limit_id == req_id
                            } // 具体 Agent ID 匹配
                            (Scope::KvNamespace(limit_ns), Scope::KvNamespace(req_ns)) => {
                                limit_ns == req_ns
                            } // KvNamespace 匹配
                            (Scope::AgentUuid(_) | Scope::KvNamespace(_), Scope::Global)
                            | (Scope::AgentUuid(_), Scope::KvNamespace(_))
                            | (Scope::KvNamespace(_), Scope::AgentUuid(_)) => false, // 具体权限不能操作其他范围
                        })
                };

                if !scope_covered {
                    continue;
                }

                if limit.permissions.contains(req_perm) {
                    is_allowed = true;
                    break;
                }
            }

            if !is_allowed {
                return Ok(false);
            }
        }
    }

    Ok(true)
}
