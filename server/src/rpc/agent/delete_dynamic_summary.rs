use crate::entity::dynamic_monitoring_summary;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::delete_common::{
    ResolvedCondition, extract_limit_and_last, resolve_conditions, scopes_from_conditions,
};
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::QueryCondition;
use nodeget_lib::permission::data_structure::{
    DynamicMonitoringSummary, Permission,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::{debug, error};

pub async fn delete_dynamic_summary(
    token: String,
    conditions: Vec<QueryCondition>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", conditions_count = conditions.len(), "delete_dynamic_summary: request received");

        let scopes = scopes_from_conditions(&conditions);
        let is_allowed = check_token_limit(
            &token_or_auth,
            scopes,
            vec![Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Delete,
            )],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Delete permission"
                    .to_owned(),
            )
            .into());
        }
        debug!(target: "monitoring", "delete_dynamic_summary: permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let (limit_count, is_last) = extract_limit_and_last(&conditions);
        let resolved_conditions = resolve_conditions(&conditions).await?;

        debug!(target: "monitoring", ?limit_count, is_last, "delete_dynamic_summary: executing delete");

        let rows_affected = if is_last || limit_count.is_some() {
            let mut query = dynamic_monitoring_summary::Entity::find();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::UuidId.eq(*uuid_id),
                        );
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp
                                .gte(*start)
                                .and(dynamic_monitoring_summary::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp.gte(*start),
                        );
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp.lte(*end),
                        );
                    }
                }
            }

            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = query
                .select_only()
                .column(dynamic_monitoring_summary::Column::Id)
                .order_by_desc(dynamic_monitoring_summary::Column::Timestamp)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    error!(target: "monitoring", error = %e, "Database query error");
                    NodegetError::DatabaseError(format!("Database query error: {e}"))
                })?;

            debug!(target: "monitoring", ids_count = ids.len(), limit, is_last, "Dynamic summary delete fetched IDs for limit/last path");

            if ids.is_empty() {
                0
            } else {
                dynamic_monitoring_summary::Entity::delete_many()
                    .filter(dynamic_monitoring_summary::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        error!(target: "monitoring", error = %e, "Database delete error");
                        NodegetError::DatabaseError(format!("Database delete error: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            let mut query = dynamic_monitoring_summary::Entity::delete_many();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::UuidId.eq(*uuid_id),
                        );
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp
                                .gte(*start)
                                .and(dynamic_monitoring_summary::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp.gte(*start),
                        );
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(
                            dynamic_monitoring_summary::Column::Timestamp.lte(*end),
                        );
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

        debug!(target: "monitoring", rows_affected = rows_affected, conditions = conditions.len(), "Dynamic monitoring summary delete completed");

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
