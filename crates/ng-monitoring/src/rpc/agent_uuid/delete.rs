use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_token::get::check_token_limit;
use serde_json::value::RawValue;
use tracing::debug;
use uuid::Uuid;

pub async fn delete_agent_uuid(token: String, agent_uuid: Uuid) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", %agent_uuid, "delete_agent_uuid: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::MonitoringUuid(MonitoringUuid::Delete)],
        )
        .await?;

        if !is_allowed {
            return Err(anyhow::anyhow!(NodegetError::PermissionDenied(
                "Permission Denied: Missing MonitoringUuid::Delete permission".to_owned(),
            )));
        }
        debug!(target: "rpc", %agent_uuid, "delete_agent_uuid: permission check passed");

        let deleted = MonitoringUuidCache::global()
            .soft_delete(agent_uuid)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to soft delete agent UUID: {e}"))
            })?;

        let json_str = if deleted {
            r#"{"success":true,"message":"Agent UUID soft-deleted"}"#.to_owned()
        } else {
            r#"{"success":false,"message":"Agent UUID not found"}"#.to_owned()
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
