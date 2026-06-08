//! `nodeget-server::database_storage` RPC 实现 — 查询主库各表存储占用

use crate::rpc::to_rpc_error;
use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use tracing::{debug, warn};

/// 排除的系统表名前缀/模式，不纳入用户存储统计
const EXCLUDED_TABLES: &[&str] = &["seaql_migrations"];

/// 表大小查询结果行
#[derive(FromQueryResult)]
struct TableSizeRow {
    /// 表名
    table_name: String,
    /// 表占用字节数
    table_size: i64,
}

/// `database_storage` RPC 响应结构
#[derive(Serialize)]
struct DatabaseStorageResponse {
    /// 各表存储占用（字节），按表名排序
    tables: BTreeMap<String, i64>,
    /// 数据库总大小（字节），所有表之和
    total: i64,
}

/// 查询主库各表存储占用，需要 super token 权限
///
/// - `token` — 认证 Token（需 super token）
/// - 返回值：包含各表大小和总计的响应
///
/// 内部步骤：
/// 1. 验证 super token 权限
/// 2. 根据数据库后端选择 `PostgreSQL` 或 `SQLite` 查询策略
/// 3. 汇总各表大小并序列化返回
///
/// # Errors
///
/// 当 Token 解析失败、认证提供者未初始化、权限不足或数据库查询失败时返回错误
pub async fn database_storage(token: String) -> jsonrpsee::core::RpcResult<Box<RawValue>> {
    debug!(target: "server", "querying database storage");
    let process_logic = async {
        // 验证 super token 权限，仅超级 Token 可查询存储信息
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = ng_core::permission::permission_checker::get_permission_checker()
            .ok_or_else(|| NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned()))?;

        let is_super = provider
            .check_super_token(&token_or_auth)
            .await
            .map_err(|e| {
                warn!(target: "db", "权限拒绝: {e}");
                NodegetError::PermissionDenied(format!("{e}"))
            })?;

        if !is_super {
            warn!(target: "db", "权限拒绝: 需要 Super Token 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Super token required".to_owned(),
            )
            .into());
        }
        debug!(target: "server", "Super token verified for database_storage");

        let db = crate::get_db().ok_or_else(|| {
            ng_core::error::NodegetError::DatabaseError("DB not initialized".to_owned())
        })?;
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

        serde_json::value::to_raw_value(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// PostgreSQL：动态发现用户表并查询各表总大小（含索引和 TOAST）
///
/// 内部步骤：
/// 1. 从 `pg_tables` 发现当前 schema 下的用户表（排除 `EXCLUDED_TABLES`）
/// 2. 使用 `unnest` + `pg_total_relation_size` 批量查询所有表大小
async fn query_postgres(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
    debug!(target: "server", "querying postgres table sizes");
    // 从 pg_tables 动态获取当前 schema 下的所有用户表
    let discover_sql = r"
        SELECT tablename AS table_name
        FROM pg_tables
        WHERE schemaname = current_schema()
          AND tablename NOT IN (SELECT unnest($1::text[]))
        ORDER BY tablename
    ";
    let excluded: Vec<String> = EXCLUDED_TABLES.iter().map(ToString::to_string).collect();
    let table_names: Vec<String> = TableNameRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        discover_sql,
        [excluded.into()],
    ))
    .all(db)
    .await
    .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
    .into_iter()
    .map(|r| r.table_name)
    .collect();

    if table_names.is_empty() {
        return Ok(BTreeMap::new());
    }

    // 使用 unnest 将表名数组展开，一次查询获取所有表大小，避免 N+1 round-trip
    let size_sql = r"
        SELECT
            t.name AS table_name,
            COALESCE(pg_total_relation_size(t.name::regclass), 0) AS table_size
        FROM unnest($1::text[]) AS t(name)
        ORDER BY t.name
    ";
    let rows = TableSizeRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        size_sql,
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

/// 表名发现查询结果行
#[derive(FromQueryResult)]
struct TableNameRow {
    /// 表名
    table_name: String,
}

/// SQLite：动态发现用户表并使用 dbstat 虚拟表查询各表占用的页面总大小
///
/// 内部步骤：
/// 1. 从 `sqlite_master` 发现所有用户表（排除 `EXCLUDED_TABLES`）
/// 2. 使用 dbstat 虚拟表批量查询各表 SUM(pgsize)
/// 3. 为 dbstat 未覆盖的空表补 0
///
/// 注意：`dbstat` 虚拟表需要 `SQLite` 编译时启用 `SQLITE_ENABLE_DBSTAT_VTAB`，
/// sqlx 的 bundled `SQLite` 默认已启用
async fn query_sqlite(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
    debug!(target: "server", "querying sqlite table sizes");
    // 从 sqlite_master 动态获取所有用户表
    let excluded: Vec<String> = EXCLUDED_TABLES.iter().map(ToString::to_string).collect();
    let placeholders: Vec<String> = excluded
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let not_in_clause = if placeholders.is_empty() {
        String::new()
    } else {
        format!(" AND name NOT IN ({})", placeholders.join(", "))
    };
    let discover_sql = format!(
        "SELECT name AS table_name FROM sqlite_master WHERE type = 'table'{not_in_clause} ORDER BY name"
    );
    let values: Vec<sea_orm::Value> = excluded.into_iter().map(std::convert::Into::into).collect();
    let table_names: Vec<String> = TableNameRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        &discover_sql,
        values,
    ))
    .all(db)
    .await
    .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
    .into_iter()
    .map(|r| r.table_name)
    .collect();

    let mut result = BTreeMap::new();
    if table_names.is_empty() {
        return Ok(result);
    }

    // 单次查询批量获取所有表大小，避免 N+1 round-trip
    let placeholders: Vec<String> = table_names
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT name AS table_name, COALESCE(SUM(pgsize), 0) AS table_size FROM dbstat WHERE name IN ({}) GROUP BY name",
        placeholders.join(", ")
    );
    let values: Vec<sea_orm::Value> = table_names.iter().map(|n| n.as_str().into()).collect();
    let rows = TableSizeRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        &sql,
        values,
    ))
    .all(db)
    .await
    .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    for row in rows {
        result.insert(row.table_name, row.table_size);
    }

    // dbstat 不包含无页面的空表，补 0 以保证所有表名都出现在结果中
    for name in &table_names {
        result.entry(name.clone()).or_insert(0);
    }
    debug!(target: "server", table_count = result.len(), "SQLite table sizes queried");

    Ok(result)
}
