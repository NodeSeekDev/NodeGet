use crate::entity::static_monitoring;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::QueryCondition;
use nodeget_lib::permission::data_structure::{Permission, Scope, StaticMonitoring};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, error};

pub async fn delete_static(
    token: String,
    conditions: Vec<QueryCondition>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", conditions_count = conditions.len(), "delete_static: request received");

        let scopes = scopes_from_conditions(&conditions);
        let is_allowed = check_token_limit(
            &token_or_auth,
            scopes,
            vec![Permission::StaticMonitoring(StaticMonitoring::Delete)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing StaticMonitoring Delete permission for requested scope"
                    .to_owned(),
            )
            .into());
        }
        debug!(target: "monitoring", "delete_static: permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global();
        let (limit_count, is_last) = extract_limit_and_last(&conditions);

        // Pre-resolve UUID conditions to uuid_ids
        let resolved_conditions: Vec<ResolvedCondition> = {
            let mut resolved = Vec::with_capacity(conditions.len());
            for cond in &conditions {
                match cond {
                    QueryCondition::Uuid(uuid) => {
                        let uuid_id = uuid_cache.get_id(uuid).await.ok_or_else(|| {
                            NodegetError::NotFound(format!("Agent UUID not found in monitoring registry: {uuid}"))
                        })?;
                        resolved.push(ResolvedCondition::UuidId(uuid_id));
                    }
                    QueryCondition::TimestampFromTo(s, e) => resolved.push(ResolvedCondition::TimestampFromTo(*s, *e)),
                    QueryCondition::TimestampFrom(s) => resolved.push(ResolvedCondition::TimestampFrom(*s)),
                    QueryCondition::TimestampTo(e) => resolved.push(ResolvedCondition::TimestampTo(*e)),
                    QueryCondition::Limit(_) | QueryCondition::Last => {}
                }
            }
            resolved
        };

        debug!(target: "monitoring", ?limit_count, is_last, "delete_static: executing delete");

        let rows_affected = if is_last || limit_count.is_some() {
            let mut query = static_monitoring::Entity::find();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(static_monitoring::Column::UuidId.eq(*uuid_id));
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            static_monitoring::Column::Timestamp
                                .gte(*start)
                                .and(static_monitoring::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(static_monitoring::Column::Timestamp.gte(*start));
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(static_monitoring::Column::Timestamp.lte(*end));
                    }
                }
            }

            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = query
                .select_only()
                .column(static_monitoring::Column::Id)
                .order_by_desc(static_monitoring::Column::Timestamp)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    error!(target: "monitoring", error = %e, "Database query error");
                    NodegetError::DatabaseError(format!("Database query error: {e}"))
                })?;

            debug!(target: "monitoring", ids_count = ids.len(), limit, is_last, "Static delete fetched IDs for limit/last path");

            if ids.is_empty() {
                0
            } else {
                static_monitoring::Entity::delete_many()
                    .filter(static_monitoring::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        error!(target: "monitoring", error = %e, "Database delete error");
                        NodegetError::DatabaseError(format!("Database delete error: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            let mut query = static_monitoring::Entity::delete_many();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(static_monitoring::Column::UuidId.eq(*uuid_id));
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            static_monitoring::Column::Timestamp
                                .gte(*start)
                                .and(static_monitoring::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(static_monitoring::Column::Timestamp.gte(*start));
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(static_monitoring::Column::Timestamp.lte(*end));
                    }
                }
            }
            query
                .exec(db)
                .await
                .map_err(|e| {
                    error!(target: "monitoring", error = %e, "Database delete error");
                    NodegetError::DatabaseError(format!("Database delete error: {e}"))
                })?
                .rows_affected
        };

        debug!(target: "monitoring", rows_affected = rows_affected, conditions = conditions.len(), "Static monitoring delete completed");

        let json_str = format!(
            "{{\"success\":true,\"deleted\":{},\"condition_count\":{}}}",
            rows_affected,
            conditions.len()
        );
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

fn scopes_from_conditions(conditions: &[QueryCondition]) -> Vec<Scope> {
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

fn extract_limit_and_last(conditions: &[QueryCondition]) -> (Option<u64>, bool) {
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
            QueryCondition::Uuid(_)
            | QueryCondition::TimestampFromTo(_, _)
            | QueryCondition::TimestampFrom(_)
            | QueryCondition::TimestampTo(_) => {}
        }
    }

    (limit_count, is_last)
}

enum ResolvedCondition {
    UuidId(i16),
    TimestampFromTo(i64, i64),
    TimestampFrom(i64),
    TimestampTo(i64),
}
