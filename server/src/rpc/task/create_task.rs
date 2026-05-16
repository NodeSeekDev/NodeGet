use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskManager;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::{TaskEvent, TaskEventType};
use nodeget_lib::utils::generate_random_string;
use sea_orm::{ActiveValue, EntityTrait, Set};
use serde_json::value::RawValue;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

pub fn validate_task_type(task_type: &TaskEventType) -> anyhow::Result<()> {
    if let TaskEventType::Execute(execute_task) = task_type
        && execute_task.cmd.trim().is_empty()
    {
        return Err(NodegetError::InvalidInput("Execute cmd cannot be empty".to_owned()).into());
    }

    Ok(())
}

pub async fn create_task(
    manager: &Arc<TaskManager>,
    token: String,
    target_uuid: Uuid,
    task_type: TaskEventType,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        validate_task_type(&task_type)?;

        let task_name = task_type.task_name();

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(target_uuid)],
            vec![Permission::Task(Task::Create(task_name.to_string()))],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(format!(
                "Permission Denied: Missing Task Create ({task_name}) permission for this Agent"
            ))
            .into());
        }

        let db = <super::TaskRpcImpl as RpcHelper>::get_db()?;
        let token = generate_random_string(10);

        let in_data = task::ActiveModel {
            id: ActiveValue::default(),
            uuid: Set(target_uuid),
            token: Set(token.clone()),
            cron_source: Set(None),
            timestamp: Set(None),
            success: Set(None),
            error_message: Set(None),
            task_event_type: <super::TaskRpcImpl as RpcHelper>::try_set_json(task_type.clone())
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?,
            task_event_result: Set(None),
        };

        debug!(target: "task", uuid = %target_uuid, "Received task");

        let result = task::Entity::insert(in_data).exec(db).await.map_err(|e| {
            error!(target: "task", error = %e, "Database insert error");
            NodegetError::DatabaseError(format!("Database insert error: {e}"))
        })?;

        let task_id = result.last_insert_id;
        debug!(target: "task", id = task_id, "Task created");

        // Ensure the uuid is registered in the monitoring_uuid table (authoritative Agent table)
        crate::monitoring_uuid_cache::MonitoringUuidCache::global()
            .get_or_insert(target_uuid)
            .await
            .ok();

        let task = TaskEvent {
            task_id: task_id.cast_unsigned(),
            task_token: token,
            task_event_type: task_type,
        };

        match manager.send_event(target_uuid, task).await {
            Ok(()) => {
                let json_str = format!("{{\"id\":{task_id}}}");
                RawValue::from_string(json_str)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
            }
            Err(e) => {
                let _ = task::Entity::delete_by_id(task_id)
                    .exec(db)
                    .await
                    .map_err(|del_err| {
                        error!(target: "task", error = %del_err, "Database delete error during rollback");
                        NodegetError::DatabaseError(format!("Database delete error: {del_err}"))
                    });
                error!(target: "task", error = %e.1, "Error sending task event");
                Err(NodegetError::AgentConnectionError(format!(
                    "Error sending task event: {}",
                    e.1
                ))
                .into())
            }
        }
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
