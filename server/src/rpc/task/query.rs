use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::query::{TaskDataQuery, TaskQueryCondition};
use nodeget_lib::utils::error_message::error_to_raw;
use nodeget_lib::utils::server_json::{rename_key, try_parse_json_field};
use sea_orm::sea_query::{Alias, BinOper, Expr};
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;

// 查询任务数据
//
// # 参数
// * `token` - 认证令牌
// * `task_data_query` - 任务数据查询条件
//
// # 返回值
// 返回查询结果的原始 JSON 值，包含 Vec<TaskResponseItem> 格式的任务数据
pub async fn query(token: String, task_data_query: TaskDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 鉴权
        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(toa) => toa,
            Err(e) => return Err((101, format!("Failed to parse token: {e}"))),
        };

        let all_task_types = [
            "ping",
            "tcp_ping",
            "http_ping",
            "web_shell",
            "execute",
            "ip",
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
            return Err((
                102,
                "Permission Denied: Insufficient permissions to read requested task types"
                    .to_string(),
            ));
        }
        let db = TaskRpcImpl::get_db()?;

        let mut query = task::Entity::find().select_only();

        query = query
            .column(task::Column::Id)
            .column(task::Column::Uuid)
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
                        // Postgres 优化：使用 JSONB 操作符
                        query = query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .binary(BinOper::Custom("?"), type_key),
                        );
                    } else {
                        // SQLite / 其他，转文本并匹配
                        let pattern = format!("%\"{type_key}\":%");
                        query = query.filter(
                            Expr::col(task::Column::TaskEventType)
                                .cast_as(Alias::new("text"))
                                .like(pattern),
                        );
                    }
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
            // Last
            query = query
                .order_by(task::Column::Timestamp, Order::Desc) // 主要按时间
                .order_by(task::Column::Id, Order::Desc) // 时间相同时按 ID
                .limit(1);
        } else if let Some(l) = limit_count {
            query = query
                .order_by(task::Column::Timestamp, Order::Desc)
                .order_by(task::Column::Id, Order::Desc)
                .limit(l);
        } else {
            // 时间正序
            query = query
                .order_by(task::Column::Timestamp, Order::Asc)
                .order_by(task::Column::Id, Order::Asc);
        }

        let mut stream = query.into_json().stream(db).await.map_err(|e| {
            error!("Database query error: {e}");
            (103, format!("Database query error: {e}"))
        })?;

        let capacity = limit_count.unwrap_or(100) as usize * 500;
        let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

        output_buffer.push(b'[');
        let mut first = true;

        while let Some(item_res) = stream.next().await {
            match item_res {
                Ok(mut v) => {
                    // 数据库是 id, struct 是 task_id
                    if let Some(obj) = v.as_object_mut() {
                        rename_key(obj, "id", "task_id");

                        // --- 修复 SQLite JSON 字符串问题 ---
                        try_parse_json_field(obj, "task_event_type");
                        try_parse_json_field(obj, "task_event_result");
                    }

                    if first {
                        first = false;
                    } else {
                        output_buffer.push(b',');
                    }

                    if let Err(e) = serde_json::to_writer(&mut output_buffer, &v) {
                        error!("Serialization failed: {e}");
                        return Err((101, format!("Serialization failed: {e}")));
                    }
                }
                Err(e) => {
                    error!("Stream read error: {e}");
                    return Err((103, format!("Stream read error: {e}")));
                }
            }
        }

        output_buffer.push(b']');

        let json_string = String::from_utf8(output_buffer).map_err(|e| {
            error!("UTF8 conversion error: {e}");
            (101, "UTF8 conversion error".to_string())
        })?;

        // 零拷贝封装
        let raw_value = RawValue::from_string(json_string).map_err(|e| {
            error!("RawValue creation error: {e}");
            (101, "RawValue creation error".to_string())
        })?;

        Ok(raw_value)
    };

    Ok(process_logic
        .await
        .unwrap_or_else(|(code, msg)| error_to_raw(code, &msg)))
}
