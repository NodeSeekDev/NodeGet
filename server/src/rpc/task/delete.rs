use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::query::TaskQueryCondition;
use sea_orm::sea_query::{Alias, BinOper, Expr, LikeExpr};
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;
use tracing::{debug, error};

/// 转义 SQL LIKE 特殊字符，防止注入攻击
fn escape_like_pattern(pattern: &str) -> String {
    pattern.replace('%', r"\%").replace('_', r"\_")
}

pub async fn delete(
    token: String,
    conditions: Vec<TaskQueryCondition>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "task", condition_count = conditions.len(), "processing task delete request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let all_task_types = [
            "ping",
            "tcp_ping",
            "http_ping",
            "http_request",
            "web_shell",
            "execute",
            "read_config",
            "edit_config",
            "ip",
            "version",
        ];

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;
        for cond in &conditions {
            if let TaskQueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }
        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let mut requested_types = Vec::new();
        for cond in &conditions {
            if let TaskQueryCondition::Type(t) = cond {
                requested_types.push(t.clone());
            }
        }

        let permissions: Vec<Permission> = if requested_types.is_empty() {
            all_task_types
                .iter()
                .map(|t| Permission::Task(Task::Delete(t.to_string())))
                .collect()
        } else {
            requested_types
                .into_iter()
                .map(|t| Permission::Task(Task::Delete(t)))
                .collect()
        };

        let is_allowed = check_token_limit(&token_or_auth, scopes, permissions).await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient permissions to delete requested task types"
                    .to_owned(),
            )
            .into());
        }

        debug!(target: "task", condition_count = conditions.len(), "Task delete permission check passed");

        let db = TaskRpcImpl::get_db()?;

        let mut select_query = task::Entity::find().select_only().column(task::Column::Id);
        let mut delete_query = task::Entity::delete_many();
        let mut is_last = false;
        let mut limit_count: Option<u64> = None;
        let condition_count = conditions.len();

        for cond in conditions {
            match cond {
                TaskQueryCondition::TaskId(id) => {
                    select_query = select_query.filter(task::Column::Id.eq(id.cast_signed()));
                    delete_query = delete_query.filter(task::Column::Id.eq(id.cast_signed()));
                }
                TaskQueryCondition::Uuid(uuid) => {
                    select_query = select_query.filter(task::Column::Uuid.eq(uuid));
                    delete_query = delete_query.filter(task::Column::Uuid.eq(uuid));
                }
                TaskQueryCondition::TimestampFromTo(start, end) => {
                    select_query = select_query.filter(
                        task::Column::Timestamp
                            .gte(start)
                            .and(task::Column::Timestamp.lte(end)),
                    );
                    delete_query = delete_query.filter(
                        task::Column::Timestamp
                            .gte(start)
                            .and(task::Column::Timestamp.lte(end)),
                    );
                }
                TaskQueryCondition::TimestampFrom(start) => {
                    select_query = select_query.filter(task::Column::Timestamp.gte(start));
                    delete_query = delete_query.filter(task::Column::Timestamp.gte(start));
                }
                TaskQueryCondition::TimestampTo(end) => {
                    select_query = select_query.filter(task::Column::Timestamp.lte(end));
                    delete_query = delete_query.filter(task::Column::Timestamp.lte(end));
                }
                TaskQueryCondition::IsSuccess => {
                    select_query = select_query.filter(task::Column::Success.eq(true));
                    delete_query = delete_query.filter(task::Column::Success.eq(true));
                }
                TaskQueryCondition::IsFailure => {
                    select_query = select_query.filter(task::Column::Success.eq(false));
                    delete_query = delete_query.filter(task::Column::Success.eq(false));
                }
                TaskQueryCondition::IsRunning => {
                    select_query = select_query.filter(task::Column::Success.is_null());
                    delete_query = delete_query.filter(task::Column::Success.is_null());
                }
                TaskQueryCondition::Type(type_key) => {
                    if db.get_database_backend() == DbBackend::Postgres {
                        select_query = select_query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .binary(BinOper::Custom("?"), type_key.clone()),
                        );
                        delete_query = delete_query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .binary(BinOper::Custom("?"), type_key),
                        );
                    } else {
                        let escaped_key = escape_like_pattern(&type_key);
                        let pattern = format!("%\"{escaped_key}\":%");
                        let like_expr = LikeExpr::new(pattern).escape('\\');
                        select_query = select_query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .cast_as(Alias::new("text"))
                                .like(like_expr.clone()),
                        );
                        delete_query = delete_query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .cast_as(Alias::new("text"))
                                .like(like_expr),
                        );
                    }
                }
                TaskQueryCondition::CronSource(cron_source) => {
                    select_query =
                        select_query.filter(task::Column::CronSource.eq(cron_source.clone()));
                    delete_query = delete_query.filter(task::Column::CronSource.eq(cron_source));
                }
                TaskQueryCondition::Limit(n) => {
                    limit_count = Some(n);
                }
                TaskQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        let rows_affected = if is_last || limit_count.is_some() {
            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = select_query
                .order_by(task::Column::Timestamp, Order::Desc)
                .order_by(task::Column::Id, Order::Desc)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    error!(target: "task", error = %e, "Database query error");
                    NodegetError::DatabaseError(format!("Database query error: {e}"))
                })?;

            if ids.is_empty() {
                0
            } else {
                task::Entity::delete_many()
                    .filter(task::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        error!(target: "task", error = %e, "Database delete error");
                        NodegetError::DatabaseError(format!("Database delete error: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            delete_query
                .exec(db)
                .await
                .map_err(|e| {
                    error!(target: "task", error = %e, "Database delete error");
                    NodegetError::DatabaseError(format!("Database delete error: {e}"))
                })?
                .rows_affected
        };

        let json_str = format!(
            "{{\"success\":true,\"deleted\":{rows_affected},\"condition_count\":{condition_count}}}"
        );

        debug!(target: "task", rows_affected, condition_count, "Task delete completed");
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let raw =
                nodeget_lib::utils::error_message::anyhow_error_to_raw(&e).unwrap_or_else(|_| {
                    RawValue::from_string(
                        r#"{"error_id":999,"error_message":"Internal error"}"#.to_owned(),
                    )
                    .unwrap_or_else(|_| RawValue::from_string("null".to_owned()).unwrap())
                });
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            let json_str = raw.get();
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                Some(json_str),
            ))
        }
    }
}
