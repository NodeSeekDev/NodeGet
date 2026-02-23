use crate::rpc::RpcHelper;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use log::info;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::value::RawValue;
use serde_json::Value;

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    #[method(name = "version")]
    async fn version(&self) -> Value;

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

    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        list_all_agent_uuid::list_all_agent_uuid(token).await
    }
}

mod list_all_agent_uuid {
    use crate::rpc::{NodegetServerRpcImpl, RpcHelper};
    use crate::token::get::check_token_limit;
    use jsonrpsee::core::RpcResult;
    use nodeget_lib::error::NodegetError;
    use nodeget_lib::permission::data_structure::{NodeGet, Permission, Scope};
    use nodeget_lib::permission::token_auth::TokenOrAuth;
    use sea_orm::{FromQueryResult, Statement};
    use serde::Serialize;
    use serde_json::value::RawValue;
    use uuid::Uuid;

    #[derive(FromQueryResult)]
    struct UuidRow {
        uuid: Uuid,
    }

    #[derive(Serialize)]
    struct ListAllAgentUuidResponse {
        uuids: Vec<Uuid>,
    }

    pub async fn list_all_agent_uuid(token: String) -> RpcResult<Box<RawValue>> {
        let process_logic = async {
            let token_or_auth = TokenOrAuth::from_full_token(&token)
                .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

            let scope = Scope::Global;
            let permission = Permission::NodeGet(NodeGet::ListAllAgentUuid);

            let is_allowed = check_token_limit(&token_or_auth, vec![scope], vec![permission]).await?;

            if !is_allowed {
                return Err(NodegetError::PermissionDenied(
                    "Permission Denied: Insufficient NodeGet ListAllAgentUuid permissions".to_owned(),
                )
                .into());
            }

            let db = NodegetServerRpcImpl::get_db()?;
            let uuids = fetch_all_agent_uuids(db).await?;

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

    async fn fetch_all_agent_uuids(
        db: &sea_orm::DatabaseConnection,
    ) -> anyhow::Result<Vec<Uuid>> {
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
