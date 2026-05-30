use crate::DB;
use crate::db_registry::{is_read_query, json_to_sea_value, row_to_json};
use crate::rpc::token_identity;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{NodeGet as NodeGetPermission, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
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

        let is_allowed = check_token_limit(
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

        let db = DB
            .get()
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

pub async fn get_database_type(token: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);
    debug!(target: "nodeget", token_key = tk, username = un, "get_database_type called");

    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
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

        let db = DB
            .get()
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
