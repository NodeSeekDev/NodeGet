pub mod config;
pub mod generic;
pub mod postgres;
pub mod utils;

use crate::DB;
use anyhow::{Context, Result};
use sea_orm::{DatabaseBackend, DatabaseConnection};

/// 清理结果统计
#[derive(Debug, Default)]
pub struct CleanupResult {
    pub static_monitoring_deleted: u64,
    pub dynamic_monitoring_deleted: u64,
    pub task_deleted: u64,
    pub crontab_result_deleted: u64,
}

/// 获取数据库连接
fn get_db() -> Result<&'static DatabaseConnection> {
    DB.get().context("DB not initialized")
}

/// 清理过期的数据
///
/// 根据 KV 表中设置的 `database_limit`_* 配置，清理各表中的过期数据
///
/// # 清理逻辑
/// - 查询每个 `agent_uuid` 的最后一条数据的时间戳
/// - 减去配置的毫秒数得到保留范围
/// - 删除该范围之外的数据
///
/// # 注意
/// KV 中的配置值和数据库中的时间戳都使用毫秒单位
///
/// # 返回值
/// 返回清理的记录数量统计
pub async fn cleanup_expired_data() -> Result<CleanupResult> {
    let db = get_db()?;

    // 根据数据库类型选择不同的清理策略
    match db.get_database_backend() {
        DatabaseBackend::Postgres => postgres::cleanup_expired_data_postgres(db).await,
        _ => generic::cleanup_expired_data_generic(db).await,
    }
}

/// 搜索数据库中 kv 表，查找满足以下条件的 UUID：
/// - kv namespace 为有效的 UUID 格式
/// - kv key 以 `database_limit_*` 开头
///
/// 对于 PostgreSQL，使用 JSONB 操作符优化查询
/// 对于 SQLite，使用内存过滤
///
/// # 返回值
/// 成功时返回满足条件的 UUID 字符串列表
pub async fn find_uuids_with_database_limit() -> Result<Vec<String>> {
    let db = get_db()?;

    // 根据数据库类型选择不同的查询策略
    match db.get_database_backend() {
        DatabaseBackend::Postgres => postgres::find_uuids_with_database_limit_postgres(db).await,
        _ => {
            // SQLite 或其他数据库使用通用方法
            generic::find_uuids_with_database_limit_generic(db).await
        }
    }
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
pub async fn find_uuids_with_database_limit_paginated(page_size: u64) -> Result<Vec<String>> {
    generic::find_uuids_with_database_limit_paginated(get_db()?, page_size).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_is_valid_uuid() {
        assert!(utils::is_valid_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!utils::is_valid_uuid("not-a-uuid"));
        assert!(!utils::is_valid_uuid(""));
    }

    #[test]
    fn test_get_limit_millis() {
        let json_num: Value = serde_json::json!(86_400_000);
        let json_str: Value = serde_json::json!("3600000");
        let json_invalid: Value = serde_json::json!("abc");

        assert_eq!(utils::get_limit_millis(&json_num), Some(86_400_000));
        assert_eq!(utils::get_limit_millis(&json_str), Some(3_600_000));
        assert_eq!(utils::get_limit_millis(&json_invalid), None);
    }
}
