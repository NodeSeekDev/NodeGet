use super::CleanupResult;
use super::config::CleanupConfig;
use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};

/// `PostgreSQL` 优化版本
pub async fn cleanup_expired_data_postgres(db: &DatabaseConnection) -> Result<CleanupResult> {
    let mut result = CleanupResult::default();

    // 获取所有需要清理的 agent UUID 及其配置
    let configs = get_cleanup_configs_postgres(db).await?;

    for config in configs {
        // 清理 static_monitoring
        if let Some(limit) = config.static_monitoring_limit {
            let deleted =
                cleanup_table_postgres(db, "static_monitoring", &config.agent_uuid, limit).await?;
            result.static_monitoring_deleted += deleted;
        }

        // 清理 dynamic_monitoring
        if let Some(limit) = config.dynamic_monitoring_limit {
            let deleted =
                cleanup_table_postgres(db, "dynamic_monitoring", &config.agent_uuid, limit).await?;
            result.dynamic_monitoring_deleted += deleted;
        }

        // 清理 task
        if let Some(limit) = config.task_limit {
            let deleted = cleanup_task_table_postgres(db, &config.agent_uuid, limit).await?;
            result.task_deleted += deleted;
        }
    }

    // 清理 crontab_result（全局表，从 global 配置读取）
    if let Some(limit) = get_global_crontab_result_limit_postgres(db).await? {
        let deleted = cleanup_crontab_result_table_postgres(db, limit).await?;
        result.crontab_result_deleted = deleted;
    }

    Ok(result)
}

/// `PostgreSQL`: 获取所有需要清理的配置
async fn get_cleanup_configs_postgres(db: &DatabaseConnection) -> Result<Vec<CleanupConfig>> {
    let sql = r"
        SELECT 
            namespace as agent_uuid,
            MAX(CASE WHEN key = 'database_limit_static_monitoring' THEN value #>> '{}' END) as static_limit,
            MAX(CASE WHEN key = 'database_limit_dynamic_monitoring' THEN value #>> '{}' END) as dynamic_limit,
            MAX(CASE WHEN key = 'database_limit_task' THEN value #>> '{}' END) as task_limit
        FROM kv
        WHERE namespace ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        AND key IN (
            'database_limit_static_monitoring',
            'database_limit_dynamic_monitoring',
            'database_limit_task'
        )
        GROUP BY namespace
    ";

    #[derive(FromQueryResult)]
    struct ConfigRow {
        agent_uuid: String,
        static_limit: Option<String>,
        dynamic_limit: Option<String>,
        task_limit: Option<String>,
    }

    let rows = ConfigRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .all(db)
    .await?;

    let configs: Vec<CleanupConfig> = rows
        .into_iter()
        .map(|row| CleanupConfig {
            agent_uuid: row.agent_uuid,
            static_monitoring_limit: row.static_limit.and_then(|s| s.parse().ok()),
            dynamic_monitoring_limit: row.dynamic_limit.and_then(|s| s.parse().ok()),
            task_limit: row.task_limit.and_then(|s| s.parse().ok()),
            crontab_result_limit: None,
        })
        .collect();

    Ok(configs)
}

/// `PostgreSQL`: 清理指定表的数据
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
async fn cleanup_table_postgres(
    db: &DatabaseConnection,
    table_name: &str,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    // 使用参数化查询避免 SQL 注入
    let sql = format!(
        r"
        DELETE FROM {table_name}
        WHERE uuid = '{agent_uuid}'
        AND timestamp < (
            SELECT MAX(timestamp) - {limit_millis}
            FROM {table_name}
            WHERE uuid = '{agent_uuid}'
        )
        "
    );

    let result = db.execute_unprepared(&sql).await?;

    Ok(result.rows_affected())
}

/// `PostgreSQL`: 清理 task 表（timestamp 可能为 NULL）
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
async fn cleanup_task_table_postgres(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    let sql = format!(
        r"
        DELETE FROM task
        WHERE uuid = '{agent_uuid}'
        AND timestamp IS NOT NULL
        AND timestamp < (
            SELECT MAX(timestamp) - {limit_millis}
            FROM task
            WHERE uuid = '{agent_uuid}'
            AND timestamp IS NOT NULL
        )
        "
    );

    let result = db.execute_unprepared(&sql).await?;

    Ok(result.rows_affected())
}

/// `PostgreSQL`: 清理 `crontab_result` 表
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
///
/// `注意：crontab_result` 是全局表，不关联特定 agent
async fn cleanup_crontab_result_table_postgres(
    db: &DatabaseConnection,
    limit_millis: i64,
) -> Result<u64> {
    let sql = format!(
        r"
        DELETE FROM crontab_result
        WHERE run_time IS NOT NULL
        AND run_time < (
            SELECT MAX(run_time) - {limit_millis}
            FROM crontab_result
            WHERE run_time IS NOT NULL
        )
        "
    );

    let result = db.execute_unprepared(&sql).await?;

    Ok(result.rows_affected())
}

/// 从 global 配置中获取 `crontab_result` 的清理限制 (`PostgreSQL` 版本)
///
/// 查找 namespace 为 `global` 且 key 为 `database_limit_crontab_result` 的 KV 记录
/// 若不存在则返回 None
async fn get_global_crontab_result_limit_postgres(db: &DatabaseConnection) -> Result<Option<i64>> {
    let sql = r"
        SELECT 
            value #>> '{}' as limit_value
        FROM kv
        WHERE namespace = 'global'
        AND key = 'database_limit_crontab_result'
        LIMIT 1
    ";

    #[derive(FromQueryResult)]
    struct LimitRow {
        limit_value: Option<String>,
    }

    let result = LimitRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .one(db)
    .await?;

    Ok(result.and_then(|row| row.limit_value.and_then(|s| s.parse().ok())))
}

pub async fn find_uuids_with_database_limit_postgres(
    db: &DatabaseConnection,
) -> Result<Vec<String>> {
    let sql = r"
        SELECT DISTINCT namespace as name
        FROM kv
        WHERE namespace ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        AND key LIKE 'database_limit_%'
        ORDER BY name ASC
    ";

    #[derive(FromQueryResult)]
    struct UuidResult {
        name: String,
    }

    let results = UuidResult::find_by_statement(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .all(db)
    .await?;

    Ok(results.into_iter().map(|r| r.name).collect())
}
