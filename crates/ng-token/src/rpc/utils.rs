//! Token RPC 子模块共享的工具函数。
//!
//! 提供 `extract_target_identifier`（标识符提取）和 `find_target_token`（目标查找），
//! 以及 `verify_supertoken`（超级令牌校验），被 change_password、roll_token_secret 等方法共用。

use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::token;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tracing::{debug, warn};

use crate::super_token::check_super_token;

/// 从标识符中提取 token_key 或 username 部分。
///
/// 支持多种输入格式：
/// - `key:secret` 格式 → 提取 key 部分（忽略 secret）
/// - `username|password` 格式 → 提取 username 部分（忽略 password）
/// - 纯 key/username → 原样返回
///
/// - `identifier`：原始标识符字符串
/// - 返回：提取后的 token_key 或 username
pub fn extract_target_identifier(identifier: &str) -> &str {
    identifier.split_once(':').map_or(
        identifier.split_once('|').map_or(identifier, |(u, _)| u),
        |(k, _)| k,
    )
}

/// 按 token_key 或 username 在数据库中查找目标 Token 记录。
///
/// 查找顺序：先按 token_key 精确匹配，再按 username 精确匹配。
/// 每次只返回一条记录（token_key 和 username 均有唯一约束）。
///
/// - `identifier`：目标标识符，支持 `key:secret`、`username|password` 或纯标识符
/// - 返回：匹配的 token::Model
/// - 错误：数据库连接未初始化、未找到匹配记录
pub async fn find_target_token(identifier: &str) -> Result<token::Model, NodegetError> {
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_owned())
    })?;

    let key_or_name = extract_target_identifier(identifier);

    // 优先按 token_key 查找
    if let Some(model) = token::Entity::find()
        .filter(token::Column::TokenKey.eq(key_or_name))
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("DB query error: {e}")))?
    {
        debug!(target: "token", target_key = %key_or_name, "found target token by token_key");
        return Ok(model);
    }

    // 回退按 username 查找
    if let Some(model) = token::Entity::find()
        .filter(token::Column::Username.eq(key_or_name))
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("DB query error: {e}")))?
    {
        debug!(target: "token", target_username = %key_or_name, "found target token by username");
        return Ok(model);
    }

    warn!(target: "token", target = %key_or_name, "target token not found by key or username");
    Err(NodegetError::NotFound(format!(
        "Target token not found by key/username: {key_or_name}"
    )))
}

/// 校验调用者是否为超级令牌，非超级令牌返回 PermissionDenied 错误。
///
/// 与 `check_super_token` 不同，此函数直接返回 Result<(), NodegetError>，
/// 适合在 RPC 方法开头用作守卫条件。
///
/// - `token`：原始凭据字符串
/// - 返回：成功为 `()`，非超级令牌时为 PermissionDenied 错误
pub async fn verify_supertoken(token: &str) -> Result<(), NodegetError> {
    let token_or_auth =
        TokenOrAuth::from_full_token(token).map_err(NodegetError::ParseError)?;

    let is_super = check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(e.to_string()))?;

    if !is_super {
        warn!(target: "token", "non-supertoken attempted privileged operation");
        return Err(NodegetError::PermissionDenied(
            "Only SuperToken can perform this operation".to_owned(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_target_identifier ────────────────────────────────────

    #[test]
    fn test_extract_target_identifier_key_secret() {
        assert_eq!(extract_target_identifier("mykey:mysecret"), "mykey");
    }

    #[test]
    fn test_extract_target_identifier_username_password() {
        assert_eq!(extract_target_identifier("admin|password123"), "admin");
    }

    #[test]
    fn test_extract_target_identifier_plain_key() {
        assert_eq!(extract_target_identifier("just_a_key"), "just_a_key");
    }

    #[test]
    fn test_extract_target_identifier_key_colon_takes_priority() {
        // "a:b|c" — colon split takes priority over pipe split
        assert_eq!(extract_target_identifier("a:b|c"), "a");
    }

    #[test]
    fn test_extract_target_identifier_empty_parts() {
        assert_eq!(extract_target_identifier(":secret"), "");
        assert_eq!(extract_target_identifier("|password"), "");
        assert_eq!(extract_target_identifier("key:"), "key");
        assert_eq!(extract_target_identifier("user|"), "user");
    }

    #[test]
    fn test_extract_target_identifier_empty_string() {
        assert_eq!(extract_target_identifier(""), "");
    }
}
