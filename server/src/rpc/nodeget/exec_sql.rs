use crate::DB;
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

        let upper = sql
            .trim_start_matches(|c| c == ' ' || c == '(' || c == ';')
            .to_uppercase();
        let is_select = upper.starts_with("SELECT")
            || upper.starts_with("PRAGMA")
            || upper.starts_with("EXPLAIN")
            || upper.starts_with("WITH");

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

fn json_to_sea_value(v: &Value) -> sea_orm::Value {
    match v {
        Value::Null => sea_orm::Value::Json(None),
        Value::Bool(b) => sea_orm::Value::Bool(Some(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                sea_orm::Value::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                sea_orm::Value::Double(Some(f))
            } else {
                sea_orm::Value::Json(Some(Box::new(serde_json::Value::String(n.to_string()))))
            }
        }
        Value::String(s) => sea_orm::Value::String(Some(s.clone())),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(v)
            .map(|s| sea_orm::Value::Json(Some(Box::new(serde_json::Value::String(s)))))
            .unwrap_or_else(|_| sea_orm::Value::String(Some(String::from("__invalid_json__")))),
    }
}

fn row_to_json(r: &sea_orm::QueryResult) -> Value {
    let columns = r.column_names();
    let mut map = serde_json::Map::new();
    for col in &columns {
        let val = try_column_as_json(r, col);
        map.insert(col.clone(), val);
    }
    Value::Object(map)
}

fn try_column_as_json(r: &sea_orm::QueryResult, col: &str) -> Value {
    if let Ok(v) = r.try_get::<Option<String>>("", col) {
        return match v {
            Some(s) => Value::String(s),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<i64>>("", col) {
        return match v {
            Some(n) => serde_json::json!(n),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<u32>>("", col) {
        return match v {
            Some(n) => serde_json::json!(n),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<f64>>("", col) {
        return match v {
            Some(n) => serde_json::json!(n),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<bool>>("", col) {
        return match v {
            Some(b) => Value::Bool(b),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<Vec<u8>>>("", col) {
        return match v {
            Some(bytes) => serde_json::json!(hex::encode(&bytes)),
            None => Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<serde_json::Value>>("", col) {
        return match v {
            Some(j) => j,
            None => Value::Null,
        };
    }
    Value::Null
}
