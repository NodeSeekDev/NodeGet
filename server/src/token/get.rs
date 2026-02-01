use crate::DB;
use crate::entity::token;
use crate::token::{hash_string, split_token};
use nodeget_lib::permission::data_structure::{Limit, Permission, Scope, Token};
use nodeget_lib::utils::get_local_timestamp_ms;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use crate::token::super_token::check_super_token;

pub async fn get_token(
    token_str: Option<String>,
    username: Option<String>,
    password: Option<String>,
) -> Result<Token, (i64, String)> {
    let db = DB
        .get()
        .ok_or((107, "Database connection not initialized".to_string()))?;
    let token_model = if let Some(full_token) = token_str {
        let (key, secret) = split_token(&full_token).map_err(|e| (101, e))?;
        let model = token::Entity::find()
            .filter(token::Column::TokenKey.eq(key))
            .one(db)
            .await
            .map_err(|e| (103, format!("Database query error: {e}")))?
            .ok_or((105, "Token key not found in database".to_string()))?;

        if model.token_hash != hash_string(secret) {
            return Err((102, "Invalid token secret".to_string()));
        }
        model
    } else if let (Some(u), Some(p)) = (username, password) {
        let model = token::Entity::find()
            .filter(token::Column::Username.eq(u))
            .one(db)
            .await
            .map_err(|e| (103, format!("Database query error: {e}")))?
            .ok_or((105, "Username not found in database".to_string()))?;
        let p_hash = hash_string(&p);
        if model.password_hash != Some(p_hash) {
            return Err((102, "Invalid password".to_string()));
        }
        model
    } else {
        return Err((101, "No authentication information provided".to_string()));
    };
    let token_limit: Vec<Limit> = serde_json::from_value(token_model.token_limit)
        .map_err(|e| (101, format!("Failed to parse token permissions: {e}")))?;
    Ok(Token {
        version: token_model.version as u8,
        token_key: token_model.token_key,
        timestamp_from: token_model.time_stamp_from,
        timestamp_to: token_model.time_stamp_to,
        token_limit,
        username: token_model.username,
    })
}

pub async fn check_token_limit(
    token_str: Option<String>,
    username: Option<String>,
    password: Option<String>,

    scopes: Vec<Scope>,
    permissions: Vec<Permission>,
) -> Result<bool, (i64, String)> {
    if let Ok(true) = check_super_token(
        token_str.as_deref(),
        username.as_deref(),
        password.as_deref(),
    )
        .await
    {
        return Ok(true);
    }

    let token = get_token(token_str, username, password).await?;

    let now = get_local_timestamp_ms() as i64;

    if let Some(from) = token.timestamp_from {
        if now < from {
            return Ok(false);
        }
    }

    if let Some(to) = token.timestamp_to {
        if now > to {
            return Ok(false);
        }
    }

    // 对于传入的每一个 Scope 和每一个 Permission，Token 中必须至少有一个 Limit 规则能够同时满足它们。
    // 即：请求的 (Scope, Permission) 必须被 Token 的 Limit 集合覆盖。

    for req_scope in &scopes {
        for req_perm in &permissions {
            let mut is_allowed = false;

            for limit in &token.token_limit {
                let scope_covered =
                    limit
                        .scopes
                        .iter()
                        .any(|limit_scope| match (limit_scope, req_scope) {
                            (Scope::Global, _) => true, // 可以操作任何具体 Scope
                            (Scope::AgentUuid(limit_id), Scope::AgentUuid(req_id)) => {
                                limit_id == req_id
                            } // 具体 Agent ID 匹配
                            (Scope::AgentUuid(_), Scope::Global) => false,
                        });

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
