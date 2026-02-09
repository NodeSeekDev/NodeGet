use crate::entity::{crontab_result, task};
use crate::rpc::RpcHelper;
use crate::rpc::task::{TaskManager, TaskRpcImpl};
use chrono::Utc;
use log::{debug, error};
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::{TaskEvent, TaskEventType};
use nodeget_lib::utils::generate_random_string;
use sea_orm::{ActiveValue, EntityTrait, Set};
use uuid::Uuid;

pub async fn crontab_task(
    cron_id: i64,
    cron_name: String,
    uuids: Vec<Uuid>,
    task_event_type: TaskEventType,
) {
    let db = match TaskRpcImpl::get_db() {
        Ok(db) => db,
        Err(e) => {
            error!(
                "Critical: Failed to get DB connection for CronJob [{cron_name}]: {e:?}"
            );
            return;
        }
    };

    for uuid in uuids {
        let process_logic = async {
            let token = generate_random_string(10);

            let in_data = task::ActiveModel {
                id: ActiveValue::default(),
                uuid: Set(uuid),
                token: Set(token.clone()),
                timestamp: Set(None),
                success: Set(None),
                error_message: Set(None),
                task_event_type: TaskRpcImpl::try_set_json(task_event_type.clone())
                    .map_err(|e| NodegetError::SerializationError(format!("{e}")))?,
                task_event_result: Set(None),
            };

            let result = task::Entity::insert(in_data).exec(db).await.map_err(|e| {
                error!("Database insert error: {e}");
                NodegetError::DatabaseError(format!("Database insert error: {e}"))
            })?;

            let task_id = result.last_insert_id;
            debug!("Inserted task with id [{task_id}]");

            let task = TaskEvent {
                task_id: task_id.cast_unsigned(),
                task_token: token,
                task_event_type: task_event_type.clone(),
            };

            let manager = TaskManager::global();

            match manager.send_event(uuid, task).await {
                Ok(()) => Ok(task_id),
                Err(e) => {
                    let _ = task::Entity::delete_by_id(task_id)
                        .exec(db)
                        .await
                        .map_err(|del_err| {
                            error!("Database delete error during rollback: {del_err}");
                            NodegetError::DatabaseError(format!("Database delete error: {del_err}"))
                        });
                    error!("Error sending task event: {}", e.1);
                    Err(NodegetError::AgentConnectionError(format!("Error sending task event: {}", e.1)))
                }
            }
        };

        // 执行逻辑并获取结果状态
        let (success, message) = match process_logic.await {
            Ok(new_id) => (
                true,
                format!(
                    "Task dispatched successfully to agent [{uuid}]. Task ID: {new_id}"
                ),
            ),
            Err(e) => {
                (
                    false,
                    format!(
                        "Failed to dispatch to agent [{uuid}]. Error: {}",
                        e
                    ),
                )
            }
        };

        let crontab_log = crontab_result::ActiveModel {
            id: ActiveValue::NotSet,
            cron_id: Set(cron_id),
            cron_name: Set(cron_name.clone()),
            run_time: Set(Some(Utc::now().timestamp_millis())),
            success: Set(Some(success)),
            message: Set(Some(message)),
        };

        if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
            error!(
                "Failed to save CrontabResult for cron [{cron_name}]: {e}"
            );
        }
    }
}
