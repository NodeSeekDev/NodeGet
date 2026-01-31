use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::{TaskManager, TaskRpcImpl};
use log::{debug, error};
use nodeget_lib::task::{TaskEvent, TaskEventResponse, TaskEventType};
use nodeget_lib::utils::error_message::generate_error_message;
use nodeget_lib::utils::generate_random_string;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, Set};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;
use crate::SERVER_CONFIG;

pub async fn create_task(
    manager: &TaskManager,
    _token: String,
    target_uuid: Uuid,
    mut task_type: TaskEventType,
) -> Value {
    let process_logic = async {
        let db = TaskRpcImpl::get_db().map_err(|e| (e.0 as u32, e.1))?;
        let token = generate_random_string(10);
        let server_config = SERVER_CONFIG.get().ok_or((107, "Config not found".to_string()))?;

        let in_data = task::ActiveModel {
            id: ActiveValue::default(),
            uuid: Set(target_uuid),
            token: Set(token.clone()),
            timestamp: Set(None),
            success: Set(None),
            error_message: Set(None),
            task_event_type: TaskRpcImpl::try_set_json(task_type.clone()).map_err(|e| (101, e))?,
            task_event_result: Set(None),
        };

        debug!("Received task for [{target_uuid}]");

        let result = task::Entity::insert(in_data).exec(db).await.map_err(|e| {
            error!("Database insert error: {e}");
            (103, format!("Database insert error: {e}"))
        })?;

        let task_id = result.last_insert_id;
        debug!("Inserted task with id [{task_id}]");

        let task = TaskEvent {
            task_id: task_id.cast_unsigned(),
            task_token: token,
            task_event_type: task_type,
        };

        match manager.send_event(target_uuid, task).await {
            Ok(()) => Ok(task_id),
            Err(e) => {
                let _ = task::Entity::delete_by_id(task_id)
                    .exec(db)
                    .await
                    .map_err(|del_err| {
                        error!("Database delete error during rollback: {del_err}");
                        (103, format!("Database delete error: {del_err}"))
                    });

                error!("Error sending task event: {}", e.1);
                Err((e.0, format!("Error sending task event: {}", e.1)))
            }
        }
    };

    match process_logic.await {
        Ok(new_id) => json!({ "id": new_id }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}

pub async fn upload_task_result(_token: String, task_response: TaskEventResponse) -> Value {
    let process_logic = async {
        let db = TaskRpcImpl::get_db().map_err(|e| (e.0 as u32, e.1))?;

        let task_model = task::Entity::find_by_id(task_response.task_id.cast_signed())
            .filter(task::Column::Uuid.eq(task_response.agent_uuid))
            .filter(task::Column::Token.eq(task_response.task_token))
            .one(db)
            .await
            .map_err(|e| {
                error!("Database query error: {e}");
                (103, format!("Database query error: {e}"))
            })?
            .ok_or_else(|| {
                (
                    105,
                    "Task validation failed: Invalid ID, UUID, or Token".to_string(),
                )
            })?;

        let mut active_model: task::ActiveModel = task_model.into();

        active_model.timestamp = Set(Some(task_response.timestamp.cast_signed()));
        active_model.success = Set(Some(task_response.success));

        active_model.error_message = Set(task_response.error_message.map(|v| {
            let json_v = serde_json::to_value(v).unwrap_or(Value::Null);
            match json_v {
                Value::String(s) => s,
                _ => json_v.to_string(),
            }
        }));

        let result_json = task_response
            .task_event_result
            .map(TaskRpcImpl::try_set_json) // Result<ActiveValue, String>
            .transpose() // Result<Option<ActiveValue>, String>
            .map_err(|e| (101, e))?;

        active_model.task_event_result =
            result_json.map_or(Set(None), |active_val| Set(Some(active_val.unwrap())));

        active_model.update(db).await.map_err(|e| {
            error!("Database update error: {e}");
            (103, format!("Database update error: {e}"))
        })?;

        debug!(
            "Task [{}] result uploaded successfully",
            task_response.task_id
        );

        Ok(true)
    };

    match process_logic.await {
        Ok(_) => json!({ "id": task_response.task_id }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}
