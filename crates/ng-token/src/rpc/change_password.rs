use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_db::entity::token;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::value::RawValue;
use tracing::debug;

use super::utils::{find_target_token, verify_supertoken};
use crate::cache::TokenCache;
use crate::hash_string;

/// 修改指定 token 的密码（Super Token 专用）
///
/// # 参数
/// - `token` — Super Token（用于鉴权）
/// - `target_token` — 目标 token 的 `token_key` 或 `username`
///   - 支持纯 `token_key`、`username`，或 `token_key:secret` 格式（secret 不校验）
/// - `new_password` — 新密码，非空且不少于 6 个字符
///
/// # 鉴权
/// 仅 Super Token 可以调用。
pub async fn change_password(
    token: String,
    target_token: String,
    new_password: String,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        verify_supertoken(&token).await?;

        if new_password.is_empty() {
            return Err(
                NodegetError::InvalidInput("New password cannot be empty".to_owned()).into(),
            );
        }
        if new_password.len() < 6 {
            return Err(NodegetError::InvalidInput(
                "New password must be at least 6 characters long".to_owned(),
            )
            .into());
        }

        let target_model = find_target_token(&target_token).await?;

        let new_password_hash = hash_string(&new_password);

        let db = ng_db::get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        let mut active_model: token::ActiveModel = target_model.into();
        active_model.password_hash = Set(Some(new_password_hash));

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to update password: {e}")))?;

        debug!(
            target: "token",
            token_key = %updated.token_key,
            username = ?updated.username,
            "password changed successfully"
        );

        if let Err(e) = TokenCache::reload().await {
            tracing::error!(
                target: "token",
                error = %e,
                "Failed to reload token cache after password change"
            );
        }

        let response = r#"{"success":true,"message":"Password changed successfully"}"#.to_owned();
        RawValue::from_string(response)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
