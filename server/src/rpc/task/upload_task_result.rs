use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskManager;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::{TaskEventResponse, TaskEventType};
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{EntityTrait, Set};
use serde_json::Value;
use serde_json::value::RawValue;
use std::sync::Arc;
use tracing::{debug, error};

pub async fn upload_task_result(
    manager: &Arc<TaskManager>,
    token: String,
    task_response: TaskEventResponse,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        // 先进行权限预检，防止无权限调用者通过数据库查询差异探测任务存在性（时序攻击）
        let has_any_task_write = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(task_response.agent_uuid)],
            vec![Permission::Task(Task::Write("*".to_string()))],
        )
        .await?;

        if !has_any_task_write {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing Task Write permission for this Agent".to_owned(),
            )
            .into());
        }

        let db = <super::TaskRpcImpl as RpcHelper>::get_db()?;

        // 只查询一次获取完整记录，避免 TOCTOU 竞态和不必要的数据库开销
        let task_model = task::Entity::find_by_id(task_response.task_id.cast_signed())
            .filter(task::Column::Uuid.eq(task_response.agent_uuid))
            .filter(task::Column::Token.eq(task_response.task_token.clone()))
            .one(db)
            .await
            .map_err(|e| {
                error!(target: "task", error = %e, "Database query error");
                NodegetError::DatabaseError(format!("Database query error: {e}"))
            })?
            .ok_or_else(|| {
                NodegetError::NotFound(
                    "Task validation failed: Invalid ID, UUID, or Token".to_owned(),
                )
            })?;

        if task_model.success.is_some() {
            return Err(NodegetError::InvalidInput(
                "Task result has already been uploaded".to_owned(),
            )
            .into());
        }

        let original_task_type: TaskEventType = serde_json::from_value(task_model.task_event_type)
            .map_err(|e| {
                NodegetError::SerializationError(format!("Failed to parse original task type: {e}"))
            })?;

        let task_name = original_task_type.task_name();

        // 精确权限检查：确认 token 对具体 task_name 有 Write 权限
        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(task_response.agent_uuid)],
            vec![Permission::Task(Task::Write(task_name.to_string()))],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(format!(
                "Permission Denied: Missing Task Write ({task_name}) permission for this Agent"
            ))
            .into());
        }

        let error_message = task_response.error_message.clone().map(|v| {
            let json_v = serde_json::to_value(v).unwrap_or(Value::Null);
            match json_v {
                Value::String(s) => s,
                _ => format!("{json_v}"),
            }
        });

        let task_event_result = task_response
            .task_event_result
            .as_ref()
            .map(|result| {
                serde_json::to_value(result).map_err(|e| {
                    NodegetError::SerializationError(format!(
                        "Failed to serialize task event result: {e}"
                    ))
                })
            })
            .transpose()
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

        let update_result = task::Entity::update_many()
            .set(task::ActiveModel {
                timestamp: Set(Some(task_response.timestamp.cast_signed())),
                success: Set(Some(task_response.success)),
                error_message: Set(error_message),
                task_event_result: Set(task_event_result),
                ..Default::default()
            })
            .filter(task::Column::Id.eq(task_response.task_id.cast_signed()))
            .filter(task::Column::Uuid.eq(task_response.agent_uuid))
            .filter(task::Column::Token.eq(task_response.task_token.clone()))
            .filter(task::Column::Success.is_null())
            .exec(db)
            .await
            .map_err(|e| {
                error!(target: "task", error = %e, "Database update error");
                NodegetError::DatabaseError(format!("Database update error: {e}"))
            })?;

        if update_result.rows_affected == 0 {
            return Err(NodegetError::InvalidInput(
                "Task result has already been uploaded".to_owned(),
            )
            .into());
        }

        manager
            .notify_blocking_waiter(task_response.task_id, task_response.clone())
            .await;

        debug!(
            target: "task",
            task_id = task_response.task_id,
            auth_type = if token_or_auth.is_auth() { "Auth" } else { "Token" },
            "Task result uploaded"
        );

        let json_str = format!("{{\"id\":{}}}", task_response.task_id);
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
