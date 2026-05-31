use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_token::get::check_token_limit;
use serde_json::value::RawValue;
use tracing::debug;

/// Response item for agent-uuid.list_all_with_agent_mode
#[derive(serde::Serialize)]
struct AgentUuidWithMode {
    uuid: uuid::Uuid,
    soft_delete: bool,
}

pub async fn list_all_agent_uuids_with_agent_mode(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", "list_all_agent_uuids_with_agent_mode: token parsed");

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
        debug!(target: "rpc", "list_all_agent_uuids_with_agent_mode: permission check passed");

        let uuids = MonitoringUuidCache::global().list_all_with_agent_mode();

        let items: Vec<AgentUuidWithMode> = uuids
            .into_iter()
            .map(|(uuid, soft_delete)| AgentUuidWithMode { uuid, soft_delete })
            .collect();

        let json_str = serde_json::to_string(&items)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

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
