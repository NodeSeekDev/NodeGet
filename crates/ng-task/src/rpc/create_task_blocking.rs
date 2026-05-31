use crate::rpc::TaskManager;
use crate::types::{TaskEvent, TaskEventType};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, Task};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::generate_random_string;
use ng_db::entity::task;
use ng_db::rpc::RpcHelper;
use sea_orm::{ActiveValue, EntityTrait, Set};
use serde_json::value::RawValue;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

/// 创建任务并阻塞等待 agent 返回结果
///
/// 与 `create_task` 的区别：
/// - `create_task` 创建任务后立即返回 `{"id": task_id}`
/// - `create_task_blocking` 创建任务后等待 agent 上传结果，然后返回完整的任务结果
/// - `如果超时（timeout_ms），返回错误`
pub async fn create_task_blocking(
    manager: &Arc<TaskManager>,
    token: String,
    target_uuid: Uuid,
    task_type: TaskEventType,
    timeout_ms: u64,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 内联 create_task 逻辑，以便在 send_event 之前注册 waiter，避免竞态

        super::create_task::validate_task_type(&task_type)?;

        let task_name = task_type.task_name();

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = crate::rpc::auth_provider()
            .ok_or_else(|| NodegetError::Other("Auth provider not initialized".to_owned()))?;

        let is_allowed = provider
            .check_token_limit(
                &token_or_auth,
                vec![Scope::AgentUuid(target_uuid)],
                vec![
                    Permission::Task(Task::Create(task_name.to_string())),
                    Permission::Task(Task::Read(task_name.to_string())),
                ],
            )
            .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(format!(
                "Permission Denied: Missing Task Create or Read ({task_name}) permission for this Agent"
            ))
            .into());
        }

        let db = crate::rpc::TaskRpcImpl::get_db()?;
        let task_token = generate_random_string(10);

        let in_data = task::ActiveModel {
            id: ActiveValue::default(),
            uuid: Set(target_uuid),
            token: Set(task_token.clone()),
            cron_source: Set(None),
            timestamp: Set(None),
            success: Set(None),
            error_message: Set(None),
            task_event_type: crate::rpc::TaskRpcImpl::try_set_json(task_type.clone())
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?,
            task_event_result: Set(None),
        };

        let result = task::Entity::insert(in_data).exec(db).await.map_err(|e| {
            error!(target: "task", error = %e, "Database insert error");
            NodegetError::DatabaseError(format!("Database insert error: {e}"))
        })?;

        let task_id = result.last_insert_id;
        let task_id_u64 = task_id.cast_unsigned();

        debug!(target: "task", task_id = task_id_u64, "task created, registering blocking waiter");

        // Ensure the uuid is registered in the monitoring_uuid table (authoritative Agent table)
        if let Some(uuid_provider) = crate::rpc::monitoring_uuid_provider() {
            let _ = uuid_provider.get_or_insert(target_uuid).await;
        }

        // 关键：在 send_event 之前注册 waiter，避免 agent 极快返回时错过通知
        let rx = manager.register_blocking_waiter(task_id_u64).await;

        let task_event = TaskEvent {
            task_id: task_id_u64,
            task_token,
            task_event_type: task_type,
        };

        if let Err(e) = manager.send_event(target_uuid, task_event).await {
            // 发送失败，清理 waiter 和 DB 记录
            manager.remove_blocking_waiter(task_id_u64).await;
            let _ = task::Entity::delete_by_id(task_id).exec(db).await;
            error!(target: "task", error = %e.1, "Error sending task event");
            return Err(NodegetError::AgentConnectionError(format!(
                "Error sending task event: {}",
                e.1
            ))
            .into());
        }

        debug!(target: "task", task_id = task_id_u64, timeout_ms = timeout_ms, "waiting for agent result");

        // 等待结果或超时
        const MAX_TIMEOUT_MS: u64 = 300_000;
        let clamped_timeout_ms = timeout_ms.min(MAX_TIMEOUT_MS);
        let timeout_duration = std::time::Duration::from_millis(clamped_timeout_ms);
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => {
                debug!(target: "task", task_id = task_id_u64, success = response.success, "blocking task completed");
                let json_str = serde_json::to_string(&response)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
                RawValue::from_string(json_str)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
            }
            Ok(Err(_)) => {
                manager.remove_blocking_waiter(task_id_u64).await;
                error!(target: "task", task_id = task_id_u64, "blocking waiter channel closed unexpectedly");
                Err(
                    NodegetError::Other("Blocking waiter channel closed unexpectedly".to_owned())
                        .into(),
                )
            }
            Err(_) => {
                manager.remove_blocking_waiter(task_id_u64).await;
                debug!(target: "task", task_id = task_id_u64, timeout_ms = timeout_ms, "blocking task timed out");
                Err(NodegetError::Other(format!(
                    "Task {task_id_u64} timed out after {timeout_ms}ms"
                ))
                .into())
            }
        }
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
