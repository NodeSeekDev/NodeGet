use super::CleanupResult;
use super::config::CleanupConfig;
use super::utils::{get_limit_millis, is_valid_uuid};
use crate::entity::{crontab_result, dynamic_monitoring, kv, static_monitoring, task};
use anyhow::Result;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// 通用版本（适用于 `SQLite`）
pub async fn cleanup_expired_data_generic(db: &DatabaseConnection) -> Result<CleanupResult> {
    let mut result = CleanupResult::default();

    // 获取所有需要清理的 agent UUID 及其配置
    let configs = get_cleanup_configs_generic(db).await?;

    for config in configs {
        // 清理 static_monitoring
        if let Some(limit) = config.static_monitoring_limit {
            let deleted = cleanup_static_monitoring_generic(db, &config.agent_uuid, limit).await?;
            result.static_monitoring_deleted += deleted;
        }

        // 清理 dynamic_monitoring
        if let Some(limit) = config.dynamic_monitoring_limit {
            let deleted = cleanup_dynamic_monitoring_generic(db, &config.agent_uuid, limit).await?;
            result.dynamic_monitoring_deleted += deleted;
        }

        // 清理 task
        if let Some(limit) = config.task_limit {
            let deleted = cleanup_task_generic(db, &config.agent_uuid, limit).await?;
            result.task_deleted += deleted;
        }
    }

    // 清理 crontab_result（全局表，从 global 配置读取）
    if let Some(limit) = get_global_crontab_result_limit(db).await? {
        let deleted = cleanup_crontab_result_generic(db, limit).await?;
        result.crontab_result_deleted = deleted;
    }

    Ok(result)
}

/// 通用版本: 获取所有需要清理的配置
async fn get_cleanup_configs_generic(db: &DatabaseConnection) -> Result<Vec<CleanupConfig>> {
    let records = kv::Entity::find()
        .filter(kv::Column::Key.is_in([
            "database_limit_static_monitoring",
            "database_limit_dynamic_monitoring",
            "database_limit_task",
        ]))
        .all(db)
        .await?;

    let mut config_map: HashMap<String, CleanupConfig> = HashMap::new();

    for record in records {
        if !is_valid_uuid(&record.namespace) {
            continue;
        }

        let Some(limit_millis) = get_limit_millis(&record.value) else {
            continue;
        };

        let config = config_map
            .entry(record.namespace.clone())
            .or_insert_with(|| CleanupConfig {
                agent_uuid: record.namespace.clone(),
                static_monitoring_limit: None,
                dynamic_monitoring_limit: None,
                task_limit: None,
                crontab_result_limit: None,
            });

        match record.key.as_str() {
            "database_limit_static_monitoring" => {
                config.static_monitoring_limit = Some(limit_millis)
            }
            "database_limit_dynamic_monitoring" => {
                config.dynamic_monitoring_limit = Some(limit_millis);
            }
            "database_limit_task" => config.task_limit = Some(limit_millis),
            _ => {}
        }
    }

    Ok(config_map.into_values().collect())
}

/// 通用版本: 清理 `static_monitoring` 表
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
async fn cleanup_static_monitoring_generic(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    let uuid = Uuid::parse_str(agent_uuid)?;

    // 获取该 agent 的最大 timestamp
    let max_timestamp: Option<i64> = static_monitoring::Entity::find()
        .filter(static_monitoring::Column::Uuid.eq(uuid))
        .select_only()
        .column(static_monitoring::Column::Timestamp)
        .order_by_desc(static_monitoring::Column::Timestamp)
        .into_tuple()
        .one(db)
        .await?;

    let Some(max_timestamp) = max_timestamp else {
        return Ok(0);
    };

    // 计算需要保留的最小 timestamp
    let min_timestamp = max_timestamp - limit_millis;

    // 删除旧数据
    let deleted = static_monitoring::Entity::delete_many()
        .filter(static_monitoring::Column::Uuid.eq(uuid))
        .filter(static_monitoring::Column::Timestamp.lt(min_timestamp))
        .exec(db)
        .await?;

    Ok(deleted.rows_affected)
}

/// 通用版本: 清理 `dynamic_monitoring` 表
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
async fn cleanup_dynamic_monitoring_generic(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    let uuid = Uuid::parse_str(agent_uuid)?;

    // 获取该 agent 的最大 timestamp
    let max_timestamp: Option<i64> = dynamic_monitoring::Entity::find()
        .filter(dynamic_monitoring::Column::Uuid.eq(uuid))
        .select_only()
        .column(dynamic_monitoring::Column::Timestamp)
        .order_by_desc(dynamic_monitoring::Column::Timestamp)
        .into_tuple()
        .one(db)
        .await?;

    let Some(max_timestamp) = max_timestamp else {
        return Ok(0);
    };

    // 计算需要保留的最小 timestamp
    let min_timestamp = max_timestamp - limit_millis;

    // 删除旧数据
    let deleted = dynamic_monitoring::Entity::delete_many()
        .filter(dynamic_monitoring::Column::Uuid.eq(uuid))
        .filter(dynamic_monitoring::Column::Timestamp.lt(min_timestamp))
        .exec(db)
        .await?;

    Ok(deleted.rows_affected)
}

/// 通用版本: 清理 task 表
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
async fn cleanup_task_generic(
    db: &DatabaseConnection,
    agent_uuid: &str,
    limit_millis: i64,
) -> Result<u64> {
    let uuid = Uuid::parse_str(agent_uuid)?;

    // 获取该 agent 的最大 timestamp（排除 NULL）
    let max_timestamp: Option<i64> = task::Entity::find()
        .filter(task::Column::Uuid.eq(uuid))
        .filter(task::Column::Timestamp.is_not_null())
        .select_only()
        .column(task::Column::Timestamp)
        .order_by_desc(task::Column::Timestamp)
        .into_tuple()
        .one(db)
        .await?;

    let Some(max_timestamp) = max_timestamp else {
        return Ok(0);
    };

    // 计算需要保留的最小 timestamp
    let min_timestamp = max_timestamp - limit_millis;

    // 删除旧数据
    let deleted = task::Entity::delete_many()
        .filter(task::Column::Uuid.eq(uuid))
        .filter(task::Column::Timestamp.is_not_null())
        .filter(task::Column::Timestamp.lt(min_timestamp))
        .exec(db)
        .await?;

    Ok(deleted.rows_affected)
}

/// 通用版本: 清理 `crontab_result` 表
///
/// # 参数
/// * `limit_millis` - 保留的毫秒数
///
/// `注意：crontab_result` 是全局表，不关联特定 agent
async fn cleanup_crontab_result_generic(db: &DatabaseConnection, limit_millis: i64) -> Result<u64> {
    // 获取 crontab_result 的最大 run_time
    let max_run_time: Option<i64> = crontab_result::Entity::find()
        .filter(crontab_result::Column::RunTime.is_not_null())
        .select_only()
        .column(crontab_result::Column::RunTime)
        .order_by_desc(crontab_result::Column::RunTime)
        .into_tuple()
        .one(db)
        .await?;

    let Some(max_run_time) = max_run_time else {
        return Ok(0);
    };

    // 计算需要保留的最小 run_time
    let min_run_time = max_run_time - limit_millis;

    // 删除旧数据
    let deleted = crontab_result::Entity::delete_many()
        .filter(crontab_result::Column::RunTime.is_not_null())
        .filter(crontab_result::Column::RunTime.lt(min_run_time))
        .exec(db)
        .await?;

    Ok(deleted.rows_affected)
}

/// 从 global 配置中获取 `crontab_result` 的清理限制
///
/// 查找 namespace 为 `global` 且 key 为 `database_limit_crontab_result` 的 KV 记录
/// 若不存在则返回 None
async fn get_global_crontab_result_limit(db: &DatabaseConnection) -> Result<Option<i64>> {
    let global_record = kv::Entity::find()
        .filter(kv::Column::Namespace.eq("global"))
        .filter(kv::Column::Key.eq("database_limit_crontab_result"))
        .one(db)
        .await?;

    Ok(global_record.and_then(|record| get_limit_millis(&record.value)))
}

/// 通用版本（适用于 `SQLite`）
pub async fn find_uuids_with_database_limit_generic(
    db: &DatabaseConnection,
) -> Result<Vec<String>> {
    // 查询所有包含 `database_limit_*` 配置的 namespace（去重）
    let all_names: Vec<String> = kv::Entity::find()
        .select_only()
        .column(kv::Column::Namespace)
        .filter(kv::Column::Key.like("database_limit_%"))
        .distinct()
        .into_tuple()
        .all(db)
        .await?;

    let uuid_names: Vec<String> = all_names
        .into_iter()
        .filter(|name| is_valid_uuid(name))
        .collect();

    Ok(uuid_names)
}

/// 搜索数据库中 kv 表，查找满足以下条件的 UUID（分页处理版本）：
/// - kv namespace 为有效的 UUID 格式
/// - kv key 以 `database_limit_*` 开头
///
/// 这个版本使用分页处理，适合处理大量数据，避免一次性加载所有记录
///
/// # 参数
/// * `page_size` - 每页处理的记录数
///
/// # 返回值
/// 成功时返回满足条件的 UUID 字符串列表
pub async fn find_uuids_with_database_limit_paginated(
    db: &DatabaseConnection,
    page_size: u64,
) -> Result<Vec<String>> {
    let mut result = HashSet::new();
    let mut paginator = kv::Entity::find()
        .filter(kv::Column::Key.like("database_limit_%"))
        .paginate(db, page_size);

    while let Some(records) = paginator.fetch_and_next().await? {
        for record in records {
            if !is_valid_uuid(&record.namespace) {
                continue;
            }

            result.insert(record.namespace);
        }
    }

    let mut output: Vec<String> = result.into_iter().collect();
    output.sort_unstable();
    Ok(output)
}
