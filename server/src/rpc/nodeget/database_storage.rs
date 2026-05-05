use super::{NodegetServerRpcImpl, RpcHelper};
use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use tracing::debug;

/// 需要查询的表名列表（排除 `seaql_migrations`）
const TABLE_NAMES: &[&str] = &[
    "monitoring_uuid",
    "static_monitoring",
    "dynamic_monitoring",
    "dynamic_monitoring_summary",
    "task",
    "token",
    "kv",
    "crontab",
    "crontab_result",
    "js_worker",
    "js_result",
];

#[derive(FromQueryResult)]
struct TableSizeRow {
    table_name: String,
    table_size: i64,
}

#[derive(Serialize)]
struct DatabaseStorageResponse {
    /// 各表存储占用（字节）
    tables: BTreeMap<String, i64>,
    /// 数据库总大小（字节），所有表之和
    total: i64,
}

pub async fn database_storage(token: String) -> RpcResult<Box<RawValue>> {
    debug!(target: "server", "querying database storage");
    let process_logic = async {
        // 验证 super token 权限
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Super token required".to_owned(),
            )
            .into());
        }
        debug!(target: "server", "Super token verified for database_storage");

        let db = NodegetServerRpcImpl::get_db()?;
        let tables = match db.get_database_backend() {
            DatabaseBackend::Postgres => query_postgres(db).await?,
            DatabaseBackend::Sqlite => query_sqlite(db).await?,
            backend => {
                return Err(NodegetError::Other(format!(
                    "Unsupported database backend: {backend:?}"
                ))
                .into());
            }
        };

        let total: i64 = tables.values().sum();
        let response = DatabaseStorageResponse { tables, total };
        debug!(target: "server", total_bytes = total, "Database storage query completed");

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

/// `PostgreSQL`: 使用 `pg_total_relation_size()` 查询各表总大小（含索引和 TOAST）
async fn query_postgres(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
    debug!(target: "server", "querying postgres table sizes");
    // 使用 unnest 将表名数组展开，一次查询获取所有表的大小
    let sql = r"
        SELECT
            t.name AS table_name,
            COALESCE(pg_total_relation_size(t.name::regclass), 0) AS table_size
        FROM unnest($1::text[]) AS t(name)
        ORDER BY t.name
    ";

    let table_names: Vec<String> = TABLE_NAMES.iter().map(ToString::to_string).collect();

    let rows = TableSizeRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [table_names.into()],
    ))
    .all(db)
    .await
    .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    let mut result = BTreeMap::new();
    for row in rows {
        result.insert(row.table_name, row.table_size);
    }
    debug!(target: "server", table_count = result.len(), "Postgres table sizes queried");

    Ok(result)
}

#[derive(FromQueryResult)]
struct SizeRow {
    table_size: i64,
}

/// `SQLite`: 使用 dbstat 虚拟表查询各表占用的页面总大小
async fn query_sqlite(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
    debug!(target: "server", "querying sqlite table sizes");
    let mut result = BTreeMap::new();

    for &table_name in TABLE_NAMES {
        // dbstat 虚拟表在 SQLite 编译时需启用 SQLITE_ENABLE_DBSTAT_VTAB
        // sqlx 的 bundled SQLite 默认启用此选项
        let sql = "SELECT COALESCE(SUM(pgsize), 0) AS table_size FROM dbstat WHERE name = ?";

        let row = SizeRow::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            [table_name.into()],
        ))
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let size = row.map_or(0, |r| r.table_size);
        result.insert(table_name.to_string(), size);
    }
    debug!(target: "server", table_count = result.len(), "SQLite table sizes queried");

    Ok(result)
}
