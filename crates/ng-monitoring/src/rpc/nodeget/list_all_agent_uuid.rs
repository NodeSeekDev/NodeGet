use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, NodeGet, Permission, Scope};
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_token::{TokenOrAuth, check_super_token, get_token};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, trace};
use uuid::Uuid;

enum AgentUuidListPermission {
    All,
    Scoped(HashSet<Uuid>),
}

#[derive(Serialize)]
struct ListAllAgentUuidResponse {
    uuids: Vec<Uuid>,
}

pub async fn list_all_agent_uuid(token: String) -> RpcResult<Box<RawValue>> {
    debug!(target: "server", "listing all agent uuids");
    let process_logic = async {
        let permission = resolve_list_agent_uuid_permission(&token).await?;

        let all_uuids = MonitoringUuidCache::global().list_all();
        let uuids = match permission {
            AgentUuidListPermission::All => all_uuids,
            AgentUuidListPermission::Scoped(allowed) => all_uuids
                .into_iter()
                .filter(|uuid| allowed.contains(uuid))
                .collect(),
        };

        let response = ListAllAgentUuidResponse { uuids };
        debug!(target: "server", uuid_count = response.uuids.len(), "list_all_agent_uuid completed");
        let json_str = serde_json::to_string(&response)
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

async fn resolve_list_agent_uuid_permission(
    token: &str,
) -> anyhow::Result<AgentUuidListPermission> {
    trace!(target: "server", "resolving agent uuid list permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_super_token = check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
    if is_super_token {
        trace!(target: "server", "Super token detected, granting All agent UUID access");
        return Ok(AgentUuidListPermission::All);
    }

    let token_info = get_token(&token_or_auth).await?;
    let now = get_local_timestamp_ms_i64()?;

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

    let mut has_global_list_permission = false;
    let mut nodeget_scoped_uuids: HashSet<Uuid> = HashSet::new();
    let mut operable_scoped_uuids: HashSet<Uuid> = HashSet::new();

    for limit in &token_info.token_limit {
        #[allow(deprecated)]
        let has_list_permission = limit.permissions.iter().any(|perm| {
            matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid))
                || matches!(perm, Permission::MonitoringUuid(MonitoringUuid::List))
        });

        if has_list_permission {
            if limit
                .scopes
                .iter()
                .any(|scope| matches!(scope, Scope::Global))
            {
                has_global_list_permission = true;
            }

            for scope in &limit.scopes {
                if let Scope::AgentUuid(uuid) = scope {
                    nodeget_scoped_uuids.insert(*uuid);
                }
            }
        }

        // "可操作" = 对该 AgentUuid Scope 至少拥有一种非 list 权限
        #[allow(deprecated)]
        let has_any_operation_permission = limit.permissions.iter().any(|perm| {
            !matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid))
                && !matches!(perm, Permission::MonitoringUuid(MonitoringUuid::List))
        });

        if has_any_operation_permission {
            for scope in &limit.scopes {
                if let Scope::AgentUuid(uuid) = scope {
                    operable_scoped_uuids.insert(*uuid);
                }
            }
        }
    }

    if has_global_list_permission {
        trace!(target: "server", "Global ListAllAgentUuid permission found, granting All access");
        return Ok(AgentUuidListPermission::All);
    }

    if nodeget_scoped_uuids.is_empty() {
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Insufficient NodeGet ListAllAgentUuid permissions".to_owned(),
        )
        .into());
    }

    let allowed_scoped_uuids: HashSet<Uuid> = nodeget_scoped_uuids
        .into_iter()
        .filter(|uuid| operable_scoped_uuids.contains(uuid))
        .collect();

    trace!(target: "server", allowed_count = allowed_scoped_uuids.len(), "Scoped agent UUID permission resolved");
    Ok(AgentUuidListPermission::Scoped(allowed_scoped_uuids))
}
