//! `task_upload_task_result` RPC 方法：Agent 上传任务执行结果

use crate::rpc::TaskManager;
use crate::types::{TaskEventResponse, TaskEventType};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, Task};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::task;
use ng_db::rpc::RpcHelper;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{EntityTrait, Set};
use serde_json::value::RawValue;
use std::sync::Arc;
use tracing::{debug, error};

/// Agent 上传任务执行结果
///
/// - `manager` — 任务广播管理器，用于通知 blocking waiter
/// - `token` — 身份令牌，需拥有对应任务类型的 `Task::Write` 权限
/// - `task_response` — 任务执行响应，包含 task_id、agent_uuid、task_token 等校验字段
///
/// 返回 `{"id": task_id}`。重复上传、校验失败均返回错误。
///
/// 权限检查分两阶段：
/// 1. 预检：检查 Token 是否对目标 Agent 持有任意 `Task::Write` 权限（防时序攻击）
/// 2. 精确检查：根据数据库中记录的 task_type 确认具体 Write 权限
///
/// 内部步骤：
/// 1. 预检权限（SuperToken 直接放行，否则检查 token_limit 中有无 Task::Write）
/// 2. 查询数据库记录，校验 task_id + agent_uuid + task_token 三元组
/// 3. 检查任务是否已上传结果（防重复上传）
/// 4. 从记录中解析 task_event_type，获取 task_name 进行精确权限检查
/// 5. 更新数据库记录：写入 timestamp、success、error_message、task_event_result
/// 6. 通知 blocking waiter（如有）
pub async fn upload_task_result(
    manager: &Arc<TaskManager>,
    token: String,
    task_response: TaskEventResponse,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = ng_core::permission::permission_checker::get_permission_checker()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned())
            })?;

        // 先进行权限预检，防止无权限调用者通过数据库查询差异探测任务存在性（时序攻击）
        // 预检逻辑：检查 token 是否对目标 Agent 持有任意 Task::Write 权限（不限具体 pattern）
        let is_super = provider
            .check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super {
            let token_data = provider
                .get_token(&token_or_auth)
                .await
                .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
            let has_any_task_write = token_data.token_limit.iter().any(|limit| {
                let scope_ok = limit.scopes.iter().any(|s| {
                    matches!(s, Scope::Global)
                        || matches!(s, Scope::AgentUuid(uuid) if *uuid == task_response.agent_uuid)
                });
                let perm_ok = limit
                    .permissions
                    .iter()
                    .any(|p| matches!(p, Permission::Task(Task::Write(_))));
                scope_ok && perm_ok
            });

            if !has_any_task_write {
                return Err(NodegetError::PermissionDenied(
                    "Permission Denied: Missing Task Write permission for this Agent".to_owned(),
                )
                .into());
            }
        }

        let db = crate::rpc::TaskRpcImpl::get_db()?;

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
        let is_allowed = provider
            .check_token_limit(
                &token_or_auth,
                &[Scope::AgentUuid(task_response.agent_uuid)],
                &[Permission::Task(Task::Write(task_name.to_string()))],
            )
            .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(format!(
                "Permission Denied: Missing Task Write ({task_name}) permission for this Agent"
            ))
            .into());
        }

        let error_message = task_response.error_message.clone();

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

        let task_id = task_response.task_id;
        let is_auth = token_or_auth.is_auth();
        manager.notify_blocking_waiter(task_id, task_response);

        debug!(
            target: "task",
            task_id,
            auth_type = if is_auth { "Auth" } else { "Token" },
            "Task result uploaded"
        );

        let json_str = format!("{{\"id\":{}}}", task_id);
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
