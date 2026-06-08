//! 删除操作的公共工具函数。
//!
//! 提取查询条件中的 Scope、Limit/Last 标记，以及将 `QueryCondition` 解析为
//! 数据库可用的 `ResolvedCondition`（将 UUID 转换为 `uuid_id`）。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::QueryCondition;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Scope;
use std::collections::HashSet;
use tracing::warn;

/// 从查询条件中提取 Scope 列表。
///
/// - 包含 UUID 条件时，为每个 UUID 生成 `Scope::AgentUuid`
/// - 无 UUID 条件时，回退为 `Scope::Global`
/// - 自动去重
pub fn scopes_from_conditions(conditions: &[QueryCondition]) -> Vec<Scope> {
    let mut seen = HashSet::new();
    let mut scopes = Vec::new();

    for cond in conditions {
        if let QueryCondition::Uuid(uuid) = cond
            && seen.insert(*uuid)
        {
            scopes.push(Scope::AgentUuid(*uuid));
        }
    }

    if scopes.is_empty() {
        scopes.push(Scope::Global);
    }

    scopes
}

/// 从查询条件中提取 Limit 值和 Last 标记。
///
/// - 返回值 — `(limit_count, is_last)`
pub fn extract_limit_and_last(conditions: &[QueryCondition]) -> (Option<u64>, bool) {
    let mut limit_count = None;
    let mut is_last = false;

    for cond in conditions {
        match cond {
            QueryCondition::Limit(n) => {
                limit_count = Some(*n);
            }
            QueryCondition::Last => {
                is_last = true;
            }
            _ => {}
        }
    }

    (limit_count, is_last)
}

/// 已解析的查询条件，UUID 已转换为数字 ID，Limit/Last 已过滤。
pub enum ResolvedCondition {
    /// UUID 对应的数字 ID
    UuidId(i16),
    /// 时间戳范围过滤（开始，结束），单位：毫秒
    TimestampFromTo(i64, i64),
    /// 时间戳起始过滤，单位：毫秒
    TimestampFrom(i64),
    /// 时间戳结束过滤，单位：毫秒
    TimestampTo(i64),
    /// 入库时间范围过滤（开始，结束），单位：毫秒
    StorageTimeFromTo(i64, i64),
    /// 入库时间起始过滤，单位：毫秒
    StorageTimeFrom(i64),
    /// 入库时间结束过滤，单位：毫秒
    StorageTimeTo(i64),
}

/// 将 `QueryCondition` 列表解析为 `ResolvedCondition` 列表。
///
/// - `conditions` — 原始查询条件
/// - 返回值 — UUID 已转换为 ID 的解析结果（Limit/Last 条件被过滤掉）
///
/// 内部步骤：
/// 1. 遍历每个条件
/// 2. UUID 条件通过 `MonitoringUuidCache` 转换为 ID
/// 3. 时间戳/入库时间条件直接透传
/// 4. Limit/Last 条件忽略（由调用方单独处理）
pub async fn resolve_conditions(
    conditions: &[QueryCondition],
) -> anyhow::Result<Vec<ResolvedCondition>> {
    let cache = MonitoringUuidCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned()))?;
    let mut resolved = Vec::new();

    for cond in conditions {
        match cond {
            QueryCondition::Uuid(uuid) => {
                let uuid_id = cache.get_id(uuid).ok_or_else(|| {
                    warn!(target: "monitoring", %uuid, "UUID 查找失败: 在监控注册表中未找到");
                    NodegetError::NotFound(format!(
                        "Agent UUID not found in monitoring registry: {uuid}"
                    ))
                })?;
                resolved.push(ResolvedCondition::UuidId(uuid_id));
            }
            QueryCondition::TimestampFromTo(from, to) => {
                resolved.push(ResolvedCondition::TimestampFromTo(*from, *to));
            }
            QueryCondition::TimestampFrom(from) => {
                resolved.push(ResolvedCondition::TimestampFrom(*from));
            }
            QueryCondition::TimestampTo(to) => {
                resolved.push(ResolvedCondition::TimestampTo(*to));
            }
            QueryCondition::StorageTimeFromTo(from, to) => {
                resolved.push(ResolvedCondition::StorageTimeFromTo(*from, *to));
            }
            QueryCondition::StorageTimeFrom(from) => {
                resolved.push(ResolvedCondition::StorageTimeFrom(*from));
            }
            QueryCondition::StorageTimeTo(to) => {
                resolved.push(ResolvedCondition::StorageTimeTo(*to));
            }
            QueryCondition::Limit(_) | QueryCondition::Last => {}
        }
    }

    Ok(resolved)
}
