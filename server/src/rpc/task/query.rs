use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use log::error;
use nodeget_lib::task::query::{TaskDataQuery, TaskQueryCondition};
use nodeget_lib::utils::error_message::generate_error_message;
use sea_orm::sea_query::{Alias, BinOper, Expr};
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, Order, QueryFilter,
    QueryOrder, QuerySelect,
};
use serde_json::{Map, Value, from_value};

pub async fn query(_token: String, data: Value) -> Value {
    let process_logic = async {
        let db = TaskRpcImpl::get_db()?;

        let query_req: TaskDataQuery = from_value(data).map_err(|e| {
            error!("Unable to parse task query data: {e}");
            (101, format!("Unable to parse task query data: {e}"))
        })?;

        let mut query = task::Entity::find();
        let mut is_last = false;

        for cond in query_req.condition {
            match cond {
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
                TaskQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        if is_last {
            query = query.order_by(task::Column::Id, Order::Desc).limit(1);
        } else {
            query = query.order_by(task::Column::Id, Order::Asc);
        }

        let models = query.all(db).await.map_err(|e| {
            error!("Database query error: {e}");
            (103, format!("Database query error: {e}"))
        })?;

        let result_list: Vec<Value> = models
            .into_iter()
            .map(|model| {
                let mut map = Map::new();

                map.insert("task_id".to_string(), Value::Number(model.id.into()));
                map.insert("uuid".to_string(), Value::String(model.uuid.to_string()));
                map.insert("token".to_string(), Value::String(model.token));

                if let Some(ts) = model.timestamp {
                    map.insert("timestamp".to_string(), Value::Number(ts.into()));
                } else {
                    map.insert("timestamp".to_string(), Value::Null);
                }

                match model.success {
                    Some(true) => map.insert("success".to_string(), Value::Bool(true)),
                    Some(false) => map.insert("success".to_string(), Value::Bool(false)),
                    None => map.insert("success".to_string(), Value::Null),
                };

                map.insert("task_event_type".to_string(), model.task_event_type);

                if let Some(res) = model.task_event_result {
                    map.insert("task_event_result".to_string(), res);
                } else {
                    map.insert("task_event_result".to_string(), Value::Null);
                }

                if let Some(err_msg) = model.error_message {
                    map.insert("error_message".to_string(), Value::String(err_msg));
                }

                Value::Object(map)
            })
            .collect();

        Ok(result_list)
    };

    match process_logic.await {
        Ok(results) => Value::Array(results),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}
