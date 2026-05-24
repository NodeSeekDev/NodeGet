//! `exec_templating`: Safe parameterized SQL execution.
//!
//! Shares core logic with `exec_sql`, supports parameterized queries
//! for preventing SQL injection of user-provided values.

use crate::db_registry::{DbExecResult, DbRegistryManager, json_to_sea_value, row_to_json};
use crate::rpc::db::auth::check_db_permission;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::Db as DbPermission;
use sea_orm::ConnectionTrait;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn exec_templating(
    token: String,
    name: String,
    sql: String,
    params: Option<serde_json::Value>,
) -> jsonrpsee::core::RpcResult<Box<RawValue>> {
    let (tk, un) = crate::rpc::token_identity(&token);

    let process_logic = async {
        check_db_permission(&token, &name, DbPermission::ExecSql).await?;

        let mgr = DbRegistryManager::global();
        let db_conn = mgr
            .get_conn(&name)
            .await
            .ok_or_else(|| NodegetError::DatabaseError(format!("Database '{name}' not found")))?;

        let sea_params = match params {
            Some(serde_json::Value::Array(arr)) => arr.iter().map(json_to_sea_value).collect(),
            Some(serde_json::Value::Null) | None => vec![],
            _ => {
                return Err(NodegetError::InvalidInput(
                    "params must be an array or null".to_owned(),
                )
                .into());
            }
        };
        let db_backend = db_conn.get_database_backend();
        let stmt = sea_orm::Statement::from_sql_and_values(db_backend, &sql, sea_params);

        let upper = sql
            .trim_start_matches(|c: char| c.is_whitespace() || c == '(' || c == ';')
            .to_uppercase();
        let is_select = upper.starts_with("SELECT")
            || upper.starts_with("PRAGMA")
            || upper.starts_with("EXPLAIN")
            || upper.starts_with("WITH");

        let result = if is_select {
            let rows = db_conn.query_all_raw(stmt).await?;
            let json_rows: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();
            let rc = json_rows.len() as u64;
            DbExecResult {
                success: true,
                data: json_rows,
                row_count: rc,
            }
        } else {
            let exec_result = db_conn.execute_raw(stmt).await?;
            DbExecResult {
                success: true,
                data: vec![],
                row_count: exec_result.rows_affected(),
            }
        };

        debug!(target: "db", token_key = tk, username = un, name = %name, sql_len = sql.len(), "exec_templating completed");

        let resp = serde_json::json!({
            "success": result.success,
            "data": result.data,
            "row_count": result.row_count,
        });

        let json_str = serde_json::to_string(&resp)?;
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
