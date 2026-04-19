use super::CleanupResult;
use super::config::CleanupConfig;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use anyhow::Result;
use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, QuerySelect, Statement, prelude::*};
use tracing::{debug, trace};

// 引入实体模块
use crate::entity::{
    crontab_result, dynamic_monitoring, dynamic_monitoring_summary, kv, static_monitoring, task,
};

/// 验证表名是否合法（防止SQL注入）
const ALLOWED_MONITORING_TABLES: &[&str] = &[
    "static_monitoring",
    "dynamic_monitoring",
    "dynamic_monitoring_summary",
];

/// `PostgreSQL` 优化版本
pub async fn cleanup_expired_data_postgres(db: &DatabaseConnection) -> Result<CleanupResult> {
    debug!(target: "db", "running PostgreSQL cleanup");
    let mut result = CleanupResult::default();

    // 获取所有需要清理的 agent UUID 及其配置
    let configs = get_cleanup_configs_postgres(db).await?;

    for config in configs {
        // 清理 static_monitoring
        if let Some(limit) = config.static_monitoring_limit {
            let deleted = cleanup_static_monitoring(db, &config.agent_uuid, limit).await?;
            result.static_monitoring += deleted;
        }

        // 清理 dynamic_monitoring
        if let Some(limit) = config.dynamic_monitoring_limit {
            let deleted = cleanup_dynamic_monitoring(db, &config.agent_uuid, limit).await?;
            result.dynamic_monitoring += deleted;
        }

        // 清理 dynamic_monitoring_summary
        if let Some(limit) = config.dynamic_monitoring_summary_limit {
            let deleted =
                cleanup_dynamic_monitoring_summary(db, &config.agent_uuid, limit).await?;
            result.dynamic_monitoring_summary += deleted;
        }

        // 清理 task
        if let Some(limit) = config.task_limit {
            let deleted = cleanup_task(db, &config.agent_uuid, limit).await?;
            result.task += deleted;
        }
    }

    // 清理 crontab_result（全局表，从 global 配置读取）
    if let Some(limit) = get_global_crontab_result_limit_postgres(db).await? {
        let deleted = cleanup_crontab_result(db, limit).await?;
        result.crontab_result = deleted;
    }

    Ok(result)
}

/// `PostgreSQL`: 获取所有需要清理的配置
/// 使用 `SeaORM` 构建查询，避免 Raw SQL
async fn get_cleanup_configs_postgres(db: &DatabaseConnection) -> Result<Vec<CleanupConfig>> {
    trace!(target: "db", "loading cleanup configs (postgres)");
    // 使用 SeaORM 构建复杂查询
    // 注意：CASE WHEN 表达式需要使用 Expr::cust 或原生 SQL
    // 这里我们使用更安全的参数化查询
    let sql = r"
        SELECT 
            namespace as agent_uuid,
            MAX(CASE WHEN key = 'database_limit_static_monitoring' THEN value #>> '{}' END) as static_limit,
            MAX(CASE WHEN key = 'database_limit_dynamic_monitoring' THEN value #>> '{}' END) as dynamic_limit,
            MAX(CASE WHEN key = 'database_limit_dynamic_monitoring_summary' THEN value #>> '{}' END) as dynamic_summary_limit,
            MAX(CASE WHEN key = 'database_limit_task' THEN value #>> '{}' END) as task_limit
        FROM kv
        WHERE namespace ~ $1
        AND key = ANY($2)
        GROUP BY namespace
    ";

    // 使用参数化查询，避免 SQL 注入
    let uuid_pattern =
        "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$".to_string();
    let keys: Vec<String> = vec![
        "database_limit_static_monitoring".to_string(),
        "database_limit_dynamic_monitoring".to_string(),
        "database_limit_dynamic_monitoring_summary".to_string(),
        "database_limit_task".to_string(),
    ];

    let rows = ConfigRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        [uuid_pattern.into(), keys.into()],
    ))
    .all(db)
    .await?;

    let configs: Vec<CleanupConfig> = rows
        .into_iter()
        .map(|row| CleanupConfig {
            agent_uuid: row.agent_uuid,
            static_monitoring_limit: row.static_limit.and_then(|s| s.parse().ok()),
            dynamic_monitoring_limit: row.dynamic_limit.and_then(|s| s.parse().ok()),
            dynamic_monitoring_summary_limit: row
                .dynamic_summary_limit
                .and_then(|s| s.parse().ok()),
            task_limit: row.task_limit.and_then(|s| s.parse().ok()),
            crontab_result_limit: None,
        })
        .collect();

    Ok(configs)
}

/// 清理 `static_monitoring` 表
/// 使用 `SeaORM` Entity 构建类型安全的查询
async fn cleanup_static_monitoring(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    trace!(target: "db", agent_uuid = %agent_uuid, "cleaning static monitoring (postgres)");
    let uuid = Uuid::parse_str(agent_uuid)?;
    let uuid_cache = MonitoringUuidCache::global();
    let uuid_id = match uuid_cache.get_id(&uuid).await {
        Some(id) => id,
        None => return Ok(0), // UUID not in monitoring_uuid table, nothing to clean
    };

    // 首先查询该 agent 的最大 timestamp
    let max_timestamp: Option<i64> = static_monitoring::Entity::find()
        .filter(static_monitoring::Column::UuidId.eq(uuid_id))
        .select_only()
        .column_as(static_monitoring::Column::Timestamp.max(), "max_ts")
        .into_tuple()
        .one(db)
        .await?;

    let max_ts = match max_timestamp {
        Some(ts) => ts,
        None => return Ok(0), // 没有数据需要清理
    };

    // 计算截止时间
    let cutoff_timestamp = max_ts - limit_millis;

    // 使用 SeaORM 执行删除
    let result = static_monitoring::Entity::delete_many()
        .filter(static_monitoring::Column::UuidId.eq(uuid_id))
        .filter(static_monitoring::Column::Timestamp.lt(cutoff_timestamp))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 清理 `dynamic_monitoring` 表
async fn cleanup_dynamic_monitoring(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    trace!(target: "db", agent_uuid = %agent_uuid, "cleaning dynamic monitoring (postgres)");
    let uuid = Uuid::parse_str(agent_uuid)?;
    let uuid_cache = MonitoringUuidCache::global();
    let uuid_id = match uuid_cache.get_id(&uuid).await {
        Some(id) => id,
        None => return Ok(0),
    };

    // 首先查询该 agent 的最大 timestamp
    let max_timestamp: Option<i64> = dynamic_monitoring::Entity::find()
        .filter(dynamic_monitoring::Column::UuidId.eq(uuid_id))
        .select_only()
        .column_as(dynamic_monitoring::Column::Timestamp.max(), "max_ts")
        .into_tuple()
        .one(db)
        .await?;

    let max_ts = match max_timestamp {
        Some(ts) => ts,
        None => return Ok(0),
    };

    let cutoff_timestamp = max_ts - limit_millis;

    let result = dynamic_monitoring::Entity::delete_many()
        .filter(dynamic_monitoring::Column::UuidId.eq(uuid_id))
        .filter(dynamic_monitoring::Column::Timestamp.lt(cutoff_timestamp))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 清理 `dynamic_monitoring_summary` 表
async fn cleanup_dynamic_monitoring_summary(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    trace!(target: "db", agent_uuid = %agent_uuid, "cleaning dynamic monitoring summary (postgres)");
    let uuid = Uuid::parse_str(agent_uuid)?;
    let uuid_cache = MonitoringUuidCache::global();
    let uuid_id = match uuid_cache.get_id(&uuid).await {
        Some(id) => id,
        None => return Ok(0),
    };

    // 首先查询该 agent 的最大 timestamp (uuid_id is smallint)
    let max_timestamp: Option<i64> = dynamic_monitoring_summary::Entity::find()
        .filter(dynamic_monitoring_summary::Column::UuidId.eq(uuid_id))
        .select_only()
        .column_as(
            dynamic_monitoring_summary::Column::Timestamp.max(),
            "max_ts",
        )
        .into_tuple()
        .one(db)
        .await?;

    let max_ts = match max_timestamp {
        Some(ts) => ts,
        None => return Ok(0),
    };

    let cutoff_timestamp = max_ts - limit_millis;

    let result = dynamic_monitoring_summary::Entity::delete_many()
        .filter(dynamic_monitoring_summary::Column::UuidId.eq(uuid_id))
        .filter(dynamic_monitoring_summary::Column::Timestamp.lt(cutoff_timestamp))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 清理 task 表（timestamp 可能为 NULL）
async fn cleanup_task(db: &DatabaseConnection, agent_uuid: &str, limit_millis: i64) -> Result<u64> {
    trace!(target: "db", agent_uuid = %agent_uuid, "cleaning tasks (postgres)");
    // 首先查询该 agent 的最大 timestamp（排除 NULL）
    let max_timestamp: Option<i64> = task::Entity::find()
        .filter(task::Column::Uuid.eq(Uuid::parse_str(agent_uuid)?))
        .filter(task::Column::Timestamp.is_not_null())
        .select_only()
        .column_as(task::Column::Timestamp.max(), "max_ts")
        .into_tuple()
        .one(db)
        .await?;

    let max_ts = match max_timestamp {
        Some(ts) => ts,
        None => return Ok(0),
    };

    let cutoff_timestamp = max_ts - limit_millis;

    let result = task::Entity::delete_many()
        .filter(task::Column::Uuid.eq(Uuid::parse_str(agent_uuid)?))
        .filter(task::Column::Timestamp.is_not_null())
        .filter(task::Column::Timestamp.lt(cutoff_timestamp))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 清理 `crontab_result` 表
/// `注意：crontab_result` 是全局表，不关联特定 agent
async fn cleanup_crontab_result(db: &DatabaseConnection, limit_millis: i64) -> Result<u64> {
    trace!(target: "db", "cleaning crontab results (postgres)");
    // 首先查询最大 run_time（排除 NULL）
    let max_run_time: Option<i64> = crontab_result::Entity::find()
        .filter(crontab_result::Column::RunTime.is_not_null())
        .select_only()
        .column_as(crontab_result::Column::RunTime.max(), "max_rt")
        .into_tuple()
        .one(db)
        .await?;

    let max_rt = match max_run_time {
        Some(rt) => rt,
        None => return Ok(0),
    };

    let cutoff_run_time = max_rt - limit_millis;

    let result = crontab_result::Entity::delete_many()
        .filter(crontab_result::Column::RunTime.is_not_null())
        .filter(crontab_result::Column::RunTime.lt(cutoff_run_time))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 从 global 配置中获取 `crontab_result` 的清理限制 (`PostgreSQL` 版本)
/// 查找 namespace 为 `global` 且 key 为 `database_limit_crontab_result` 的 KV 记录
/// 若不存在则返回 None
async fn get_global_crontab_result_limit_postgres(db: &DatabaseConnection) -> Result<Option<i64>> {
    trace!(target: "db", "reading global crontab result limit (postgres)");
    // 使用 SeaORM 构建查询
    let result = kv::Entity::find()
        .filter(kv::Column::Namespace.eq("global"))
        .filter(kv::Column::Key.eq("database_limit_crontab_result"))
        .select_only()
        .column(kv::Column::Value)
        .one(db)
        .await?;

    Ok(result.and_then(|row| row.value.as_str().and_then(|s| s.parse().ok())))
}

/// 查找具有数据库清理限制的 UUID
pub async fn find_uuids_with_database_limit_postgres(
    db: &DatabaseConnection,
) -> Result<Vec<String>> {
    trace!(target: "db", "finding UUIDs with database limits (postgres)");
    // 使用 SeaORM 构建查询
    // 注意：正则匹配需要使用原生 SQL
    let sql = r"
        SELECT DISTINCT namespace as name
        FROM kv
        WHERE namespace ~ $1
        AND key LIKE $2
        ORDER BY name ASC
    ";

    let uuid_pattern =
        "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";
    let key_pattern = "database_limit_%";

    let results = UuidResult::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        [uuid_pattern.into(), key_pattern.into()],
    ))
    .all(db)
    .await?;

    Ok(results.into_iter().map(|r| r.name).collect())
}

// ============================================================================
// 数据结构定义
// ============================================================================

#[derive(FromQueryResult)]
struct ConfigRow {
    agent_uuid: String,
    static_limit: Option<String>,
    dynamic_limit: Option<String>,
    dynamic_summary_limit: Option<String>,
    task_limit: Option<String>,
}

#[derive(FromQueryResult)]
struct UuidResult {
    name: String,
}
