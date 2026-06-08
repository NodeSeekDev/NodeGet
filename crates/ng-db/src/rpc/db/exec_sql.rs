//! `db.exec_sql` RPC 实现 — 在用户数据库上执行 SQL

use crate::db_registry::{DbRegistryManager, json_to_sea_value, row_to_json};
use crate::rpc::db::auth::check_db_permission;
use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Db as DbPermission;
use sea_orm::ConnectionTrait;
use serde_json::value::RawValue;
use tracing::debug;

/// `exec_sql` 核心逻辑，同时被 `nodeget::exec_sql` 复用
///
/// - `db_name` — 目标数据库名称
/// - `sql` — 待执行的 SQL 语句
/// - `params` — SQL 参数，须为 JSON 数组或 null
/// - `token` — 认证 Token
/// - 返回值：包含 `data`、`row_count`、`truncated` 的响应
///
/// 内部步骤：
/// 1. 检查 `Db::ExecSql` 权限
/// 2. 从连接池获取数据库连接
/// 3. 解析参数为 `SeaORM` `Value` 数组
/// 4. 执行 SQL 并收集结果行
/// 5. 超过 10000 行时截断并标记 `truncated: true`
///
/// 注意：使用 `query_all_raw` 统一执行所有 SQL 类型（SELECT/DML/DDL/PRAGMA/CTE），
/// 无 RETURNING 子句的 DML 返回 `data: []` + `row_count: 0`
pub async fn exec_sql_inner(
    db_name: &str,
    sql: &str,
    params: Option<serde_json::Value>,
    token: &str,
) -> anyhow::Result<Box<RawValue>> {
    check_db_permission(token, db_name, DbPermission::ExecSql).await?;

    let mgr = DbRegistryManager::global().ok_or_else(|| {
        NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
    })?;
    let db_conn = mgr
        .get_conn(db_name)
        .ok_or_else(|| NodegetError::DatabaseError(format!("Database '{db_name}' not found")))?;

    let sea_params = match params {
        Some(serde_json::Value::Array(arr)) => arr.iter().map(json_to_sea_value).collect(),
        Some(serde_json::Value::Null) | None => vec![],
        _ => {
            return Err(
                NodegetError::InvalidInput("params must be an array or null".to_owned()).into(),
            );
        }
    };

    let db_backend = db_conn.get_database_backend();
    let stmt = sea_orm::Statement::from_sql_and_values(db_backend, sql, sea_params);

    let mut rows = db_conn.query_all_raw(stmt).await?;
    let total_count = rows.len() as u64;
    let truncated = rows.len() > 10_000;
    if truncated {
        rows.truncate(10_000);
    }
    let json_rows: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();

    let (tk, un) = token_identity(token);
    debug!(target: "db", token_key = tk, username = un, name = %db_name, sql_len = sql.len(), total_count, truncated, "exec_sql");

    let resp = serde_json::json!({
        "success": true,
        "data": json_rows,
        "row_count": total_count,
        "truncated": truncated,
    });

    serde_json::value::to_raw_value(&resp)
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
}

/// `db.exec_sql` RPC 入口
///
/// - `token` — 认证 Token
/// - `name` — 目标数据库名称
/// - `sql` — SQL 语句
/// - `params` — 参数数组或 null
/// - 返回值：`RpcResult<Box<RawValue>>`
pub async fn exec_sql(
    token: String,
    name: String,
    sql: String,
    params: Option<serde_json::Value>,
) -> RpcResult<Box<RawValue>> {
    match exec_sql_inner(&name, &sql, params, &token).await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
