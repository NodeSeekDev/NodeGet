use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::{TaskManager, TaskRpcImpl};
use crate::token::get::check_token_limit;
use log::{debug, error};
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::task::{TaskEvent, TaskEventResponse, TaskEventType};
use nodeget_lib::utils::error_message::generate_error_message;
use nodeget_lib::utils::generate_random_string;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, Set};
use serde_json::{Value, json};
use uuid::Uuid;
use nodeget_lib::permission::token_auth::TokenOrAuth;

// 创建任务并将其发送给目标 Agent
// 
// # 参数
// * `manager` - 任务管理器引用
// * `token` - 认证令牌
// * `target_uuid` - 目标 Agent 的 UUID
// * `task_type` - 任务事件类型
// 
// # 返回值
// 返回创建任务的结果，成功时包含任务 ID，失败时包含错误信息
pub async fn create_task(
    manager: &TaskManager,
    token: String,
    target_uuid: Uuid,
    task_type: TaskEventType,
) -> Value {
    let process_logic = async {
        // 鉴权
        let task_name = match &task_type {
            TaskEventType::Ping(_) => "ping",
            TaskEventType::TcpPing(_) => "tcp_ping",
            TaskEventType::HttpPing(_) => "http_ping",
            TaskEventType::WebShell(_) => "web_shell",
            TaskEventType::Execute(_) => "execute",
            TaskEventType::Ip => "ip",
        };

        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(toa) => {
                toa
            }
            Err(e) => {
                return Err((101, format!("Failed to parse token: {e}")))
            }
        };

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(target_uuid)],
            vec![Permission::Task(Task::Create(task_name.to_string()))],
        )
        .await?;

        if !is_allowed {
            return Err((
                102,
                format!(
                    "Permission Denied: Missing Task Create ({task_name}) permission for this Agent"
                ),
            ));
        }

        // 查询
        let db = TaskRpcImpl::get_db()?;
        let token = generate_random_string(10);

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
                Err((i64::from(e.0), format!("Error sending task event: {}", e.1)))
            }
        }
    };

    match process_logic.await {
        Ok(new_id) => json!({ "id": new_id }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}

// 上传任务执行结果到数据库
// 
// # 参数
// * `token` - 认证令牌
// * `task_response` - 任务事件响应
// 
// # 返回值
// 返回上传结果，成功时包含任务 ID，失败时包含错误信息
pub async fn upload_task_result(token: String, task_response: TaskEventResponse) -> Value {
    let process_logic = async {
        let db = TaskRpcImpl::get_db()?;

        // 对 task_id agent_uuid  task_token 合法性验证
        let task_model = task::Entity::find_by_id(task_response.task_id.cast_signed())
            .filter(task::Column::Uuid.eq(task_response.agent_uuid))
            .filter(task::Column::Token.eq(task_response.task_token.clone()))
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

        let original_task_type: TaskEventType =
            serde_json::from_value(task_model.task_event_type.clone())
                .map_err(|e| (101, format!("Failed to parse original task type: {e}")))?;

        let task_name = match &original_task_type {
            TaskEventType::Ping(_) => "ping",
            TaskEventType::TcpPing(_) => "tcp_ping",
            TaskEventType::HttpPing(_) => "http_ping",
            TaskEventType::WebShell(_) => "web_shell",
            TaskEventType::Execute(_) => "execute",
            TaskEventType::Ip => "ip",
        };

        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(toa) => {
                toa
            }
            Err(e) => {
                return Err((101, format!("Failed to parse token: {e}")))
            }
        };

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(task_response.agent_uuid)],
            vec![Permission::Task(Task::Write(task_name.to_string()))],
        )
        .await?;

        if !is_allowed {
            return Err((
                102,
                format!(
                    "Permission Denied: Missing Task Write ({task_name}) permission for this Agent"
                ),
            ));
        }

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
            .map(TaskRpcImpl::try_set_json)
            .transpose()
            .map_err(|e| (101, e))?;

        active_model.task_event_result =
            result_json.map_or(Set(None), |active_val| Set(Some(active_val.unwrap())));

        active_model.update(db).await.map_err(|e| {
            error!("Database update error: {e}");
            (103, format!("Database update error: {e}"))
        })?;

        debug!(
            "Task [{}] result uploaded successfully by auth identifying as {:?}",
            task_response.task_id,
            if token_or_auth.is_auth() { "Auth" } else { "Token" }
        );

        Ok(task_response.task_id)
    };

    match process_logic.await {
        Ok(id) => json!({ "id": id }),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}
