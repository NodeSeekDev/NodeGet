use crate::monitoring_uuid_cache::MonitoringUuidCache;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::QueryCondition;
use nodeget_lib::permission::data_structure::Scope;
use std::collections::HashSet;

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

pub enum ResolvedCondition {
    UuidId(i16),
    TimestampFromTo(i64, i64),
    TimestampFrom(i64),
    TimestampTo(i64),
}

pub async fn resolve_conditions(
    conditions: &[QueryCondition],
) -> anyhow::Result<Vec<ResolvedCondition>> {
    let cache = MonitoringUuidCache::global();
    let mut resolved = Vec::new();

    for cond in conditions {
        match cond {
            QueryCondition::Uuid(uuid) => {
                let uuid_id = cache.get_id(uuid).await.ok_or_else(|| {
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
            QueryCondition::Limit(_) | QueryCondition::Last => {}
        }
    }

    Ok(resolved)
}
