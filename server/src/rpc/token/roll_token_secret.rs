use crate::entity::token;
use crate::token::cache::TokenCache;
use crate::token::hash_string;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::utils::generate_random_string;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::value::RawValue;
use super::utils::{find_target_token, verify_supertoken};
use tracing::debug;

/// 重新生成目标 token 的 secret（Super Token 专用）
///
/// # 参数
/// - `token` — Super Token（用于鉴权）
/// - `target_token` — 目标 token 的 `token_key` 或 `username`
///   - 支持纯 `token_key`、`username`，或 `token_key:secret` 格式（secret 不校验）
///
/// # 返回值
/// 成功时返回新的 `token_secret`：
/// ```json
/// {"key":"<token_key>","secret":"<new_secret>"}
/// ```
///
/// # 鉴权
/// 仅 Super Token 可以调用。
pub async fn roll_token_secret(token: String, target_token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        verify_supertoken(&token).await?;

        let target_model = find_target_token(&target_token).await?;

        let new_secret = generate_random_string(32);
        let new_token_hash = hash_string(&new_secret);

        let db = crate::DB.get().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        let mut active_model: token::ActiveModel = target_model.into();
        active_model.token_hash = Set(new_token_hash);

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to update token secret: {e}")))?;

        debug!(
            target: "token",
            token_key = %updated.token_key,
            username = ?updated.username,
            "token secret rolled successfully"
        );

        if let Err(e) = TokenCache::reload().await {
            tracing::error!(
                target: "token",
                error = %e,
                "Failed to reload token cache after rolling token secret"
            );
        }

        let response = format!(
            r#"{{"key":"{}","secret":"{}"}}"#,
            updated.token_key, new_secret
        );
        RawValue::from_string(response)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
