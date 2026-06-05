//! `token_edit` RPC 方法实现。
//!
//! 编辑指定令牌的权限限制，仅超级令牌可调用。

use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Limit;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::token;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::super_token::check_super_token;

/// 编辑指定令牌的权限限制（仅超级令牌可调用）。
///
/// - `token_input`：超级令牌凭据（用于鉴权）
/// - `target_token`：目标令牌的 token_key 或 username
/// - `limit`：新的权限限制列表，将完全替换原有 token_limit
/// - 返回：成功时为 `{"success":true,"id":N,"token_key":"..."}`
/// - 错误：鉴权失败、目标不存在、数据库错误
///
/// 内部步骤：
/// 1. 验证调用者为超级令牌
/// 2. 按 token_key 或 username 查找目标记录
/// 3. 序列化新的 Limit 列表并更新数据库
/// 4. 刷新 TokenCache
pub async fn edit(
    token_input: String,
    target_token: String,
    limit: Vec<Limit>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "token", target_token = %target_token, "processing token edit request");
        let token_or_auth = TokenOrAuth::from_full_token(&token_input)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super_token {
            warn!(target: "token", "non-supertoken attempted to edit token limits");
            return Err(NodegetError::PermissionDenied(
                "Only SuperToken can edit token limits".to_owned(),
            )
            .into());
        }

        debug!(target: "token", target_token = %target_token, "Super token verified, finding target token");

        let db = ng_db::get_db().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let model = if let Some(model) = token::Entity::find()
            .filter(token::Column::TokenKey.eq(&target_token))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Database query error: {e}")))?
        {
            model
        } else if let Some(model) = token::Entity::find()
            .filter(token::Column::Username.eq(&target_token))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Database query error: {e}")))?
        {
            model
        } else {
            return Err(NodegetError::NotFound(format!(
                "Token not found by key/username: {target_token}"
            ))
            .into());
        };

        debug!(target: "token", id = model.id, token_key = %model.token_key, "Target token found for editing");

        let mut active_model: token::ActiveModel = model.into();
        active_model.token_limit = Set(serde_json::to_value(limit).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize token limit: {e}"))
        })?);

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Database update error: {e}")))?;

        // 编辑成功后刷新缓存，使新权限限制立即生效
        if let Err(e) = TokenCache::reload().await {
            tracing::error!(target: "token", error = %e, "Failed to reload token cache after edit");
        }

        debug!(target: "token", id = updated.id, token_key = %updated.token_key, "Token edited successfully");

        let response = serde_json::json!({
            "success": true,
            "id": updated.id,
            "token_key": updated.token_key
        });

        serde_json::value::to_raw_value(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
