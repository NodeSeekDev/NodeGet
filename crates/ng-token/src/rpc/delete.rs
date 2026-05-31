use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::token as token_entity;
use sea_orm::{ColumnTrait, DeleteResult, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::super_token::check_super_token;

// 删除令牌的方法
async fn delete_token_by_key(token_key: String) -> Result<DeleteResult, sea_orm::DbErr> {
    debug!(target: "token", %token_key, "Deleting token by key");
    let Some(db) = ng_db::get_db() else {
        return Err(sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_owned(),
        )));
    };

    let delete_result = token_entity::Entity::delete_many()
        .filter(token_entity::Column::Id.ne(1))
        .filter(token_entity::Column::TokenKey.eq(&token_key))
        .exec(db)
        .await?;

    if delete_result.rows_affected > 0 {
        debug!(target: "token", %token_key, rows_affected = delete_result.rows_affected, "Token deleted by key");
        if let Err(e) = TokenCache::reload().await {
            tracing::error!(target: "token", error = %e, "Failed to reload token cache after delete_by_key");
        }
    } else {
        debug!(target: "token", %token_key, "No token found to delete by key");
    }

    Ok(delete_result)
}

// 根据用户名删除令牌的方法
async fn delete_token_by_username(username: String) -> Result<DeleteResult, sea_orm::DbErr> {
    debug!(target: "token", %username, "Deleting token by username");
    let Some(db) = ng_db::get_db() else {
        return Err(sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_owned(),
        )));
    };

    let delete_result = token_entity::Entity::delete_many()
        .filter(token_entity::Column::Id.ne(1))
        .filter(token_entity::Column::Username.eq(&username))
        .exec(db)
        .await?;

    if delete_result.rows_affected > 0 {
        debug!(target: "token", %username, rows_affected = delete_result.rows_affected, "Token deleted by username");
        if let Err(e) = TokenCache::reload().await {
            tracing::error!(target: "token", error = %e, "Failed to reload token cache after delete_by_username");
        }
    } else {
        debug!(target: "token", %username, "No token found to delete by username");
    }

    Ok(delete_result)
}

pub async fn delete(token: String, target_token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "token", target_token = %target_token, "processing token delete request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super_token {
            warn!(target: "token", "non-supertoken attempted to delete token");
            return Err(NodegetError::PermissionDenied(
                "Only SuperToken can delete tokens".to_owned(),
            )
            .into());
        }

        debug!(target: "token", target_token = %target_token, "Super token verified, proceeding with delete");

        if target_token.trim().is_empty() {
            return Err(
                NodegetError::InvalidInput("target_token cannot be empty".to_string()).into(),
            );
        }
        let target_token_to_delete = target_token;

        let db = ng_db::get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;
        let super_record = token_entity::Entity::find_by_id(1)
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Database error: {e}")))?
            .ok_or_else(|| {
                NodegetError::NotFound("Super Token record (ID 1) not found in database".to_owned())
            })?;

        debug!(target: "token", target = %target_token_to_delete, "Super record found, checking if target is super token");

        let target_is_super_by_key = target_token_to_delete == super_record.token_key;
        let target_is_super_by_username =
            super_record.username.as_deref() == Some(target_token_to_delete.as_str());

        if target_is_super_by_key || target_is_super_by_username {
            warn!(target: "token", "attempted to delete the super token");
            return Err(
                NodegetError::PermissionDenied("SuperToken cannot be deleted".to_owned()).into(),
            );
        }

        let delete_result_by_key = delete_token_by_key(target_token_to_delete.clone())
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let json_str = if delete_result_by_key.rows_affected > 0 {
            debug!(target: "token", target = %target_token_to_delete, matched_by = "token_key", "Token deleted successfully");
            format!(
                "{{\"message\":\"Token {} deleted successfully by SuperToken\",\"rows_affected\":{},\"matched_by\":\"token_key\"}}",
                target_token_to_delete, delete_result_by_key.rows_affected
            )
        } else {
            let delete_result_by_username =
                delete_token_by_username(target_token_to_delete.clone())
                    .await
                    .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

            if delete_result_by_username.rows_affected > 0 {
                debug!(target: "token", target = %target_token_to_delete, matched_by = "username", "Token deleted successfully");
                format!(
                    "{{\"message\":\"Token {} deleted successfully by SuperToken\",\"rows_affected\":{},\"matched_by\":\"username\"}}",
                    target_token_to_delete, delete_result_by_username.rows_affected
                )
            } else {
                return Err(NodegetError::NotFound(format!(
                    "Token not found by key/username: {target_token_to_delete}"
                ))
                .into());
            }
        };

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
