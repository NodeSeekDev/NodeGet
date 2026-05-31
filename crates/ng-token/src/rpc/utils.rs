use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::token;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tracing::{debug, warn};

use crate::super_token::check_super_token;

/// 辅助函数：从标识符中提取 token_key（如果是 `key:secret` 格式则取 key 部分）
/// 也支持 `username|password` 格式（取 username 部分）
pub fn extract_target_identifier(identifier: &str) -> &str {
    identifier.split_once(':').map_or(
        identifier.split_once('|').map_or(identifier, |(u, _)| u),
        |(k, _)| k,
    )
}

/// 辅助函数：按 token_key 或 username 查找目标 token
pub async fn find_target_token(identifier: &str) -> Result<token::Model, NodegetError> {
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::DatabaseError("Database connection not initialized".to_owned())
    })?;

    let key_or_name = extract_target_identifier(identifier);

    // 先按 token_key 查
    if let Some(model) = token::Entity::find()
        .filter(token::Column::TokenKey.eq(key_or_name))
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("DB query error: {e}")))?
    {
        debug!(target: "token", target_key = %key_or_name, "found target token by token_key");
        return Ok(model);
    }

    // 再按 username 查
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

/// 校验调用者是否为 Super Token
pub async fn verify_supertoken(token: &str) -> Result<(), NodegetError> {
    let token_or_auth =
        TokenOrAuth::from_full_token(token).map_err(|e| NodegetError::ParseError(e.to_string()))?;

    let is_super = check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if !is_super {
        warn!(target: "token", "non-supertoken attempted privileged operation");
        return Err(NodegetError::PermissionDenied(
            "Only SuperToken can perform this operation".to_owned(),
        ));
    }

    Ok(())
}
