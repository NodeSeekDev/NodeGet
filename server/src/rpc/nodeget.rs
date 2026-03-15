use crate::SERVER_CONFIG;
use crate::rpc::RpcHelper;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use log::info;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::Value;
use serde_json::value::RawValue;

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    #[method(name = "version")]
    async fn version(&self) -> Value;

    #[method(name = "uuid")]
    async fn uuid(&self) -> String;

    #[method(name = "list_all_agent_uuid")]
    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>>;
}

pub struct NodegetServerRpcImpl;

impl RpcHelper for NodegetServerRpcImpl {}

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    async fn hello(&self) -> String {
        info!("Hello Request");
        "NodeGet Server Is Running!".to_string()
    }

    async fn version(&self) -> Value {
        info!("Version Request");
        serde_json::to_value(NodeGetVersion::get()).unwrap()
    }

    async fn uuid(&self) -> String {
        info!("Uuid Request");
        SERVER_CONFIG
            .get()
            .map_or_else(String::new, |cfg| cfg.server_uuid.to_string())
    }

    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        list_all_agent_uuid::list_all_agent_uuid(token).await
    }
}

mod list_all_agent_uuid {
    use crate::rpc::{NodegetServerRpcImpl, RpcHelper};
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
        let token_or_auth = TokenOrAuth::from_full_token(token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
        if is_super_token {
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

        Ok(AgentUuidListPermission::Scoped(allowed_scoped_uuids))
    }

    async fn fetch_all_agent_uuids(db: &sea_orm::DatabaseConnection) -> anyhow::Result<Vec<Uuid>> {
        // 使用 UNION 合并三个表的查询，数据库层面去重，效率最高
        // UNION 自动去重，UNION ALL 不去重
        let sql = r"
            SELECT uuid FROM static_monitoring
            UNION
            SELECT uuid FROM dynamic_monitoring
            UNION
            SELECT uuid FROM task
            ORDER BY uuid
        ";

        let db_backend = db.get_database_backend();
        let statement = Statement::from_string(db_backend, sql.to_string());

        let rows = UuidRow::find_by_statement(statement)
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let uuids: Vec<Uuid> = rows.into_iter().map(|row| row.uuid).collect();

        Ok(uuids)
    }
}
