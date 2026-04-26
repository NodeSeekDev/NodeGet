use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use crate::token::get::check_token_limit;
use futures_util::StreamExt;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::query::{TaskDataQuery, TaskQueryCondition};
use nodeget_lib::utils::server_json::{rename_key, try_parse_json_field};
use sea_orm::sea_query::{Alias, BinOper, Expr, LikeExpr};
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;
use tracing::{debug, error};

/// 转义 SQL LIKE 特殊字符，防止注入攻击
///
/// SQL LIKE 中 `%` 匹配任意字符序列，`_` 匹配单个字符
/// 这些字符需要转义才能进行精确匹配
fn escape_like_pattern(pattern: &str) -> String {
    pattern.replace('%', r"\%").replace('_', r"\_")
}

pub async fn query(token: String, task_data_query: TaskDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "task", condition_count = task_data_query.condition.len(), "processing task query request");
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
        for cond in &task_data_query.condition {
            if let TaskQueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }
        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let mut requested_types = Vec::new();
        for cond in &task_data_query.condition {
            if let TaskQueryCondition::Type(t) = cond {
                requested_types.push(t.clone());
            }
        }

        let permissions: Vec<Permission> = if requested_types.is_empty() {
            all_task_types
                .iter()
                .map(|t| Permission::Task(Task::Read(t.to_string())))
                .collect()
        } else {
            requested_types
                .into_iter()
                .map(|t| Permission::Task(Task::Read(t)))
                .collect()
        };

        let is_allowed = check_token_limit(&token_or_auth, scopes, permissions).await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient permissions to read requested task types"
                    .to_owned(),
            )
            .into());
        }
        debug!(target: "task", condition_count = task_data_query.condition.len(), "Task query permission check passed");

        let db = TaskRpcImpl::get_db()?;

        let mut query = task::Entity::find().select_only();

        query = query
            .column(task::Column::Id)
            .column(task::Column::Uuid)
            .column(task::Column::CronSource)
            .column(task::Column::Timestamp)
            .column(task::Column::Success)
            .column(task::Column::ErrorMessage)
            .column(task::Column::TaskEventType)
            .column(task::Column::TaskEventResult);

        let mut is_last = false;
        let mut limit_count: Option<u64> = None;

        for cond in task_data_query.condition {
            match cond {
                TaskQueryCondition::TaskId(id) => {
                    query = query.filter(task::Column::Id.eq(id.cast_signed()));
                }

                TaskQueryCondition::Uuid(uuid) => {
                    query = query.filter(task::Column::Uuid.eq(uuid));
                }
                TaskQueryCondition::TimestampFromTo(start, end) => {
                    query = query.filter(
                        task::Column::Timestamp
                            .gte(start)
                            .and(task::Column::Timestamp.lte(end)),
                    );
                }
                TaskQueryCondition::TimestampFrom(start) => {
                    query = query.filter(task::Column::Timestamp.gte(start));
                }
                TaskQueryCondition::TimestampTo(end) => {
                    query = query.filter(task::Column::Timestamp.lte(end));
                }
                TaskQueryCondition::IsSuccess => {
                    query = query.filter(task::Column::Success.eq(true));
                }
                TaskQueryCondition::IsFailure => {
                    query = query.filter(task::Column::Success.eq(false));
                }
                TaskQueryCondition::IsRunning => {
                    query = query.filter(task::Column::Success.is_null());
                }
                TaskQueryCondition::Type(type_key) => {
                    if db.get_database_backend() == DbBackend::Postgres {
                        // PostgreSQL 使用 JSONB 操作符，无需转义
                        query = query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .binary(BinOper::Custom("?"), type_key),
                        );
                    } else {
                        // SQLite: 转义 LIKE 特殊字符防止注入
                        let escaped_key = escape_like_pattern(&type_key);
                        let pattern = format!("%\"{escaped_key}\":%");
                        let like_expr = LikeExpr::new(pattern).escape('\\');
                        query = query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .cast_as(Alias::new("text"))
                                .like(like_expr),
                        );
                    }
                }
                TaskQueryCondition::CronSource(cron_source) => {
                    query = query.filter(task::Column::CronSource.eq(cron_source));
                }

                TaskQueryCondition::Limit(n) => {
                    limit_count = Some(n);
                }

                TaskQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        if is_last {
            query = query
                .order_by(task::Column::Timestamp, Order::Desc)
                .order_by(task::Column::Id, Order::Desc)
                .limit(1);
        } else if let Some(l) = limit_count {
            query = query
                .order_by(task::Column::Timestamp, Order::Desc)
                .order_by(task::Column::Id, Order::Desc)
                .limit(l);
        } else {
            query = query
                .order_by(task::Column::Timestamp, Order::Asc)
                .order_by(task::Column::Id, Order::Asc);
        }

        let mut stream = query.into_json().stream(db).await.map_err(|e| {
            error!(target: "task", error = %e, "Database query error");
            NodegetError::DatabaseError(format!("Database query error: {e}"))
        })?;

        let capacity = limit_count.unwrap_or(100) as usize * 500;
        let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

        output_buffer.push(b'[');
        let mut first = true;
        let mut result_count: usize = 0;

        while let Some(item_res) = stream.next().await {
            match item_res {
                Ok(mut v) => {
                    result_count += 1;
                    if let Some(obj) = v.as_object_mut() {
                        rename_key(obj, "id", "task_id");
                        try_parse_json_field(obj, "task_event_type");
                        try_parse_json_field(obj, "task_event_result");
                    }

                    if first {
                        first = false;
                    } else {
                        output_buffer.push(b',');
                    }

                    if let Err(e) = serde_json::to_writer(&mut output_buffer, &v) {
                        error!(target: "task", error = %e, "Serialization failed");
                        return Err(NodegetError::SerializationError(format!(
                            "Serialization failed: {e}"
                        ))
                        .into());
                    }
                }
                Err(e) => {
                    error!(target: "task", error = %e, "Stream read error");
                    return Err(
                        NodegetError::DatabaseError(format!("Stream read error: {e}")).into(),
                    );
                }
            }
        }

        output_buffer.push(b']');

        debug!(target: "task", result_count, "Task query completed");

        let json_string = String::from_utf8(output_buffer).map_err(|e| {
            error!(target: "task", error = %e, "UTF8 conversion error");
            NodegetError::SerializationError("UTF8 conversion error".to_owned())
        })?;

        let raw_value = RawValue::from_string(json_string).map_err(|e| {
            error!(target: "task", error = %e, "RawValue creation error");
            NodegetError::SerializationError("RawValue creation error".to_owned())
        })?;

        Ok(raw_value)
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
