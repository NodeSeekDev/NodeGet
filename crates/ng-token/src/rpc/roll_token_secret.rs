//! `token_roll_token_secret` RPC 方法实现。
//!
//! 重新生成指定 Token 的 secret，仅超级令牌可调用。
//! 轮换后旧 secret 立即失效。

use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::utils::generate_random_string;
use ng_db::entity::token;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::value::RawValue;
use tracing::debug;

use super::utils::{find_target_token, verify_supertoken};
use crate::cache::TokenCache;
use crate::hash_string;

/// 重新生成目标 Token 的 secret（仅超级令牌可调用）。
///
/// - `token`：超级令牌凭据（用于鉴权）
/// - `target_token`：目标 Token 的 `token_key` 或 `username`；
///   支持纯 `token_key`、`username`，或 `token_key:secret` 格式（secret 不校验）
/// - 返回：成功时为 `{"key":"<token_key>","secret":"<new_secret>"}`
/// - 错误：鉴权失败、目标不存在、数据库错误
///
/// 内部步骤：
/// 1. 验证调用者为超级令牌
/// 2. 查找目标 Token 记录
/// 3. 生成新的 32 字符随机 secret，计算哈希
/// 4. 更新数据库中的 token_hash
/// 5. 刷新 TokenCache
pub async fn roll_token_secret(token: String, target_token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        verify_supertoken(&token).await?;

        let target_model = find_target_token(&target_token).await?;

        let new_secret = generate_random_string(32);
        let new_token_hash = hash_string(&new_secret);

        let db = ng_db::get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        let mut active_model: token::ActiveModel = target_model.into();
        active_model.token_hash = Set(new_token_hash);

        let updated = active_model.update(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to update token secret: {e}"))
        })?;

        debug!(
            target: "token",
            token_key = %updated.token_key,
            username = ?updated.username,
            "token secret rolled successfully"
        );

        // 轮换成功后刷新缓存，使旧 secret 立即失效
        if let Err(e) = TokenCache::reload().await {
            tracing::error!(
                target: "token",
                error = %e,
                "Failed to reload token cache after rolling token secret"
            );
        }

        serde_json::value::to_raw_value(&serde_json::json!({
            "key": updated.token_key,
            "secret": new_secret
        }))
        .map_err(|e| NodegetError::from(e).into())
    };

    // 统一错误转换：anyhow → NodegetError → JSON-RPC ErrorObject
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
