use crate::entity::task;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::TaskEventType;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

// 检查 Agent 是否有权连接终端
//
// # 参数
// * `agent_uuid` - Agent 的 UUID
// * `task_token` - 任务令牌
// * `task_id` - 任务 ID
//
// # 返回值
// 返回布尔值表示是否有权连接，失败时返回错误
pub async fn check_agent(
    agent_uuid: String,
    task_token: String,
    task_id: u64,
) -> anyhow::Result<bool> {
    let agent_uuid = Uuid::parse_str(&agent_uuid)
        .map_err(|_| NodegetError::ParseError("Invalid Agent UUID format".to_owned()))?;

    let db = TaskRpcImpl::get_db()?;

    // 查询任务记录并验证任务类型
    let task_model = task::Entity::find()
        .filter(task::Column::Id.eq(task_id.cast_signed()))
        .filter(task::Column::Uuid.eq(agent_uuid))
        .filter(task::Column::Token.eq(task_token))
        .filter(task::Column::TaskEventResult.is_null())
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("Database error: {e}")))?;

    let Some(task_model) = task_model else {
        return Ok(false);
    };

    // 解析任务类型并验证是否为 WebShell
    let task_event_type: TaskEventType = serde_json::from_value(task_model.task_event_type)
        .map_err(|e| NodegetError::SerializationError(format!("Failed to parse task_event_type: {e}")))?;

    if !matches!(task_event_type, TaskEventType::WebShell(_)) {
        return Err(NodegetError::PermissionDenied(
            "Terminal connection is only allowed for WebShell tasks".to_owned()
        ).into());
    }

    Ok(true)
}
