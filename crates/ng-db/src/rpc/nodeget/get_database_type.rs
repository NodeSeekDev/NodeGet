use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{NodeGet as NodeGetPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use sea_orm::DbBackend;
use serde_json::value::RawValue;
use tracing::warn;

pub async fn get_database_type(token: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);
    tracing::debug!(target: "nodeget", token_key = tk, username = un, "get_database_type called");

    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = crate::rpc::auth_provider()
            .ok_or_else(|| NodegetError::Other("Auth provider not initialized".to_owned()))?;

        let is_allowed = provider
            .check_token_limit(
                &token_or_auth,
                vec![Scope::Global],
                vec![Permission::NodeGet(NodeGetPermission::ExecSql)],
            )
            .await?;

        if !is_allowed {
            warn!(target: "nodeget", token_key = tk, username = un, "get_database_type permission denied");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: missing nodeget.exec_sql permission".to_owned(),
            )
            .into());
        }

        let db = crate::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("Database not initialized".to_owned()))?;

        let db_type = match db.get_database_backend() {
            DbBackend::Sqlite => "sqlite",
            DbBackend::Postgres => "postgres",
            DbBackend::MySql => "mysql",
            _ => "unknown",
        };

        let response = serde_json::json!({
            "success": true,
            "data": db_type,
        });

        let json_str = serde_json::to_string(&response)?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
