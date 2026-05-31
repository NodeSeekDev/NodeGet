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

        // Reload cache after editing token
        if let Err(e) = TokenCache::reload().await {
            tracing::error!(target: "token", error = %e, "Failed to reload token cache after edit");
        }

        debug!(target: "token", id = updated.id, token_key = %updated.token_key, "Token edited successfully");

        let response = serde_json::json!({
            "success": true,
            "id": updated.id,
            "token_key": updated.token_key
        });

        let json_str = serde_json::to_string(&response).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize response: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
