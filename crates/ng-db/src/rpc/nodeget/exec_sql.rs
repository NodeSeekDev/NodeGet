use crate::db_registry::{is_read_query, json_to_sea_value, row_to_json};
use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{NodeGet as NodeGetPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use sea_orm::{ConnectionTrait, Statement};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, warn};

pub async fn exec_sql(
    token: String,
    sql: String,
    params: Option<Value>,
) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);
    debug!(target: "nodeget", token_key = tk, username = un, sql_len = sql.len(), "exec_sql called");

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
            warn!(target: "nodeget", token_key = tk, username = un, "exec_sql permission denied");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: missing nodeget.exec_sql permission".to_owned(),
            )
            .into());
        }

        let db = crate::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("Database not initialized".to_owned()))?;

        let db_backend = db.get_database_backend();
        let sea_values = match params {
            Some(Value::Array(arr)) => arr.iter().map(json_to_sea_value).collect(),
            Some(Value::Null) | None => vec![],
            _ => {
                return Err(
                    NodegetError::InvalidInput("params must be an array".to_owned()).into(),
                );
            }
        };

        let stmt = Statement::from_sql_and_values(db_backend, &sql, sea_values);

        let is_select = is_read_query(&sql);

        let response = if is_select {
            let rows = db.query_all_raw(stmt).await?;
            let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();

            serde_json::json!({
                "success": true,
                "data": json_rows,
                "row_count": json_rows.len(),
            })
        } else {
            let result = db.execute_raw(stmt).await?;
            serde_json::json!({
                "success": true,
                "data": [],
                "row_count": result.rows_affected(),
            })
        };

        let json_str = serde_json::to_string(&response)?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
