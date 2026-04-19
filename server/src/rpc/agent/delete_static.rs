use crate::entity::static_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::delete_common::{
    ResolvedCondition, extract_limit_and_last, resolve_conditions, scopes_from_conditions,
};
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::QueryCondition;
use nodeget_lib::permission::data_structure::{Permission, StaticMonitoring};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
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
        let (limit_count, is_last) = extract_limit_and_last(&conditions);
        let resolved_conditions = resolve_conditions(&conditions).await?;

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
