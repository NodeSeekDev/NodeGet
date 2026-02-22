use crate::token;
use crate::token::get::get_token;
use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;

pub async fn delete(token: String, target_token_key: Option<String>) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let current_token_info = get_token(&token_or_auth).await?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        let json_str = if is_super_token {
            let Some(target_key_to_delete) = target_token_key else {
                return Err(NodegetError::PermissionDenied(
                    "Target token key is required for SuperToken deletion".to_string(),
                )
                .into());
            };

            let delete_result = token::delete_token_by_key(target_key_to_delete.clone())
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

            if delete_result.rows_affected > 0 {
                format!(
                    "{{\"success\":true,\"message\":\"Token {} deleted successfully by SuperToken\",\"rows_affected\":{}}}",
                    target_key_to_delete, delete_result.rows_affected
                )
            } else {
                format!(
                    "{{\"success\":false,\"message\":\"Token {target_key_to_delete} not found\"}}"
                )
            }
        } else {
            if target_token_key.is_some() {
                return Err(NodegetError::PermissionDenied(
                    "Insufficient permission to delete other tokens".to_owned(),
                )
                .into());
            }

            let target_key_to_delete = current_token_info.token_key.clone();

            let delete_result = token::delete_token_by_key(target_key_to_delete.clone())
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

            if delete_result.rows_affected > 0 {
                format!(
                    "{{\"success\":true,\"message\":\"Own token deleted successfully\",\"rows_affected\":{}}}",
                    delete_result.rows_affected
                )
            } else {
                "{\"success\":false,\"message\":\"Own token not found\"}".to_string()
            }
        };

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
