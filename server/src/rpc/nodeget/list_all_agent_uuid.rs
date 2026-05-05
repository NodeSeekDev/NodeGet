use super::{NodegetServerRpcImpl, RpcHelper};
use crate::token::get::get_token;
use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{NodeGet, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use sea_orm::{FromQueryResult, Statement};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, trace};
use uuid::Uuid;

#[derive(FromQueryResult)]
struct UuidRow {
    uuid: Uuid,
}

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

        let db = NodegetServerRpcImpl::get_db()?;
        let all_uuids = fetch_all_agent_uuids(db).await?;
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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
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
        let has_list_permission = limit
            .permissions
            .iter()
            .any(|perm| matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid)));

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

        // "可操作" = 对该 AgentUuid Scope 至少拥有一种非 NodeGet::ListAllAgentUuid 的权限
        let has_any_operation_permission = limit
            .permissions
            .iter()
            .any(|perm| !matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid)));

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

async fn fetch_all_agent_uuids(db: &sea_orm::DatabaseConnection) -> anyhow::Result<Vec<Uuid>> {
    debug!(target: "server", "fetching all agent uuids");

    let db_backend = db.get_database_backend();

    // Use EXISTS with indexed lookups on uuid_id instead of full-table UNION scan.
    // Each subquery hits the (uuid_id, timestamp) composite index, O(agent_count × 3 index hits).
    let sql = r"
        SELECT uuid FROM monitoring_uuid WHERE
          EXISTS (SELECT 1 FROM static_monitoring WHERE uuid_id = monitoring_uuid.id LIMIT 1) OR
          EXISTS (SELECT 1 FROM dynamic_monitoring WHERE uuid_id = monitoring_uuid.id LIMIT 1) OR
          EXISTS (SELECT 1 FROM dynamic_monitoring_summary WHERE uuid_id = monitoring_uuid.id LIMIT 1)
    ";
    let rows = UuidRow::find_by_statement(Statement::from_string(db_backend, sql.to_string()))
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    let mut uuid_set = std::collections::BTreeSet::<Uuid>::new();
    for row in rows {
        uuid_set.insert(row.uuid);
    }

    // Also include UUIDs from task table (not covered by monitoring tables)
    let sql = r"SELECT uuid FROM task";
    let statement = Statement::from_string(db_backend, sql.to_string());
    let rows = UuidRow::find_by_statement(statement)
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    for row in rows {
        uuid_set.insert(row.uuid);
    }

    let uuids: Vec<Uuid> = uuid_set.into_iter().collect();
    debug!(target: "server", uuid_count = uuids.len(), "Fetched all agent UUIDs");

    Ok(uuids)
}
