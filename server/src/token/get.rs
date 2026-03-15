use crate::DB;
use crate::entity::token;
use crate::token::hash_string;
use crate::token::super_token::check_super_token;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{CrontabResult, Kv, Limit, Permission, Scope, Token};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use serde_json::Value;

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

    let token_limit = parse_token_limit_with_compat(token_model.token_limit)?;

    Ok(Token {
        version: token_model.version,
        token_key: token_model.token_key,
        timestamp_from: token_model.time_stamp_from,
        timestamp_to: token_model.time_stamp_to,
        token_limit,
        username: token_model.username,
    })
}

fn drop_unknown_permissions(mut token_limit_value: Value) -> Value {
    let Some(limits) = token_limit_value.as_array_mut() else {
        return token_limit_value;
    };

    for limit in limits.iter_mut() {
        let Some(perms) = limit.get_mut("permissions").and_then(Value::as_array_mut) else {
            continue;
        };

        perms.retain(|perm| serde_json::from_value::<Permission>(perm.clone()).is_ok());
    }

    token_limit_value
}

pub fn parse_token_limit_with_compat(token_limit_value: Value) -> anyhow::Result<Vec<Limit>> {
    match serde_json::from_value::<Vec<Limit>>(token_limit_value.clone()) {
        Ok(v) => Ok(v),
        Err(original_err) => {
            let filtered = drop_unknown_permissions(token_limit_value);
            serde_json::from_value::<Vec<Limit>>(filtered).map_err(|e| {
                NodegetError::SerializationError(format!(
                    "Failed to parse token permissions: {e}; original error: {original_err}"
                ))
                .into()
            })
        }
    }
}

fn wildcard_matches_pattern(value: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| value == pattern, |prefix| value.starts_with(prefix))
}

fn permission_matches(granted: &Permission, required: &Permission) -> bool {
    if granted == required {
        return true;
    }

    match (granted, required) {
        (Permission::Kv(Kv::Read(pattern)), Permission::Kv(Kv::Read(key)))
        | (Permission::Kv(Kv::Write(pattern)), Permission::Kv(Kv::Write(key)))
        | (Permission::Kv(Kv::Delete(pattern)), Permission::Kv(Kv::Delete(key))) => {
            wildcard_matches_pattern(key, pattern)
        }
        (
            Permission::CrontabResult(CrontabResult::Read(pattern)),
            Permission::CrontabResult(CrontabResult::Read(cron_name)),
        )
        | (
            Permission::CrontabResult(CrontabResult::Delete(pattern)),
            Permission::CrontabResult(CrontabResult::Delete(cron_name)),
        ) => wildcard_matches_pattern(cron_name, pattern),
        _ => false,
    }
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

                if limit
                    .permissions
                    .iter()
                    .any(|perm| permission_matches(perm, req_perm))
                {
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
