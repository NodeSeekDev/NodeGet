use crate::crontab::toggle_crontab_enable_by_name;
use crate::token::get::get_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Crontab as CrontabPermission, Permission};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use serde_json::value::RawValue;

pub async fn toggle_enable(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let token_info = get_token(&token_or_auth).await?;

        let now = get_local_timestamp_ms_i64()
            .map_err(|e| NodegetError::Other(format!("Failed to get timestamp: {e}")))?;

        if let Some(from) = token_info.timestamp_from
            && now < from
        {
            return Err(NodegetError::PermissionDenied("Token is not yet valid".to_owned()).into());
        }

        if let Some(to) = token_info.timestamp_to
            && now > to
        {
            return Err(NodegetError::PermissionDenied("Token has expired".to_owned()).into());
        }

        let has_crontab_write_permission = token_info.token_limit.iter().any(|limit| {
            limit
                .permissions
                .iter()
                .any(|perm| matches!(perm, Permission::Crontab(CrontabPermission::Write)))
        });

        if !has_crontab_write_permission {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient Crontab Write permission".to_owned(),
            )
            .into());
        }

        let new_state = toggle_crontab_enable_by_name(name)
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to toggle crontab: {e}")))?;

        let json_str = match new_state {
            Some(state) => format!("{{\"success\":true,\"enabled\":{}}}", state),
            None => "{\"success\":false,\"message\":\"Crontab not found\"}".to_string(),
        };

        RawValue::from_string(json_str)
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
