use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{MonitoringUuid, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn list_all_agent_uuids(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", "list_all_agent_uuids: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::MonitoringUuid(MonitoringUuid::List)],
        )
        .await?;

        if !is_allowed {
            return Err(anyhow::anyhow!(NodegetError::PermissionDenied(
                "Permission Denied: Missing MonitoringUuid::List permission".to_owned(),
            )));
        }
        debug!(target: "rpc", "list_all_agent_uuids: permission check passed");

        let uuids = crate::monitoring_uuid_cache::MonitoringUuidCache::global()
            .list_all()
            .await;

        let json_str = serde_json::to_string(&uuids)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

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
