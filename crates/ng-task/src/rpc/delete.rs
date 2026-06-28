//! `task_delete` RPC 方法：按条件删除任务记录

use super::escape_like_pattern;
use crate::types::query::TaskQueryCondition;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, Task};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::task;
use ng_db::rpc::RpcHelper;
use sea_orm::sea_query::{Alias, BinOper, Expr, LikeExpr};
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;
use tracing::{debug, error};

/// 按条件删除任务记录
///
/// - `token` — 身份令牌，需拥有对应任务类型的 `Task::Delete` 权限
/// - `conditions` — 删除条件列表，各条件之间为 AND 关系
///
/// 返回 `{"success":true,"deleted":N,"condition_count":N}`。
/// 当使用 `Last` 或 `Limit` 条件时，先查询匹配的 ID 再按 ID 删除。
/// 删除成功后会尝试刷新 MonitoringUuid 缓存。
///
/// 内部步骤：
/// 1. 解析 Token，收集查询条件中的 UUID 构建 scope，确定需要的 Delete 权限
/// 2. 检查权限：无 UUID 条件时使用 Global scope
/// 3. 构建查询和删除语句，处理各条件类型
/// 4. 若有 `Last` 或 `Limit` 条件，先查 ID 再按 ID 删除
/// 5. 刷新 MonitoringUuid 缓存（删除可能影响关联数据）
pub async fn delete(
    token: String,
    conditions: Vec<TaskQueryCondition>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "task", condition_count = conditions.len(), "processing task delete request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        // 任务类型白名单：必须与 `TaskEventType::task_name()`（types/mod.rs）逐项对齐，
        // 否则未指定 Type 条件时枚举鉴权会漏掉某个类型。
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
            "dns",
            "self_update",
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

        let provider = ng_core::permission::permission_checker::get_permission_checker()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned())
            })?;

        let is_allowed = provider
            .check_token_limit(&token_or_auth, &scopes, &permissions)
            .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient permissions to delete requested task types"
                    .to_owned(),
            )
            .into());
        }

        debug!(target: "task", condition_count = conditions.len(), "Task delete permission check passed");

        let db = crate::rpc::TaskRpcImpl::get_db()?;

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
                    // 钳制上限，避免 select 出海量 id 致 Vec<i64> OOM。与 crontab_result 对齐。
                    const MAX_LIMIT: u64 = 10_000;
                    limit_count = Some(std::cmp::min(n, MAX_LIMIT));
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

        // 注：task 表与 monitoring_uuid 表无任何关联（task 实体 Relation 为空），
        // 删除 task 不会影响 monitoring_uuid 记录，无需 reload 该缓存。

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let raw = ng_core::utils::error_message::anyhow_error_to_raw(&e).unwrap_or_else(|_| {
                RawValue::from_string(
                    r#"{"error_id":999,"error_message":"Internal error"}"#.to_owned(),
                )
                .unwrap_or_else(|_| RawValue::from_string("null".to_owned()).unwrap())
            });
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            let json_str = raw.get();
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                Some(json_str),
            ))
        }
    }
}
