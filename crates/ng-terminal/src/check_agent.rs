//! Agent Terminal 连接授权校验模块。
//!
//! 职责：验证 Agent 发起的 Terminal WebSocket 连接是否对应一个有效的 WebShell 任务。
//! 通过查询 `task` 表确认 task_id / agent_uuid / task_token 三元组匹配，
//! 且任务类型为 WebShell、尚未完成（`TaskEventResult` 为 null）。

use ng_core::error::NodegetError;
use ng_db::entity::task;
use ng_task::TaskEventType;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tracing::{debug, trace, warn};
use uuid::Uuid;

/// 校验 Agent 是否有权通过 Terminal 连接。
///
/// - `agent_uuid` - Agent 的 UUID 字符串
/// - `task_token` - 任务分配时下发的 Token，用于验证请求来源合法
/// - `task_id` - 任务 ID
///
/// 返回：
/// - `Ok(true)` — 任务存在、未完成、类型为 WebShell
/// - `Ok(false)` — 查无此任务记录
/// - `Err` — UUID 格式错误、数据库异常、或任务类型不是 WebShell
///
/// 内部步骤：
/// 1. 解析 agent_uuid 为 [`Uuid`]，格式不合法时返回 ParseError
/// 2. 获取数据库连接
/// 3. 按 id + uuid + token + TaskEventResult 为 null 四条件查询任务记录
/// 4. 未找到记录时返回 `Ok(false)`
/// 5. 解析 task_event_type 字段，验证是否为 [`TaskEventType::WebShell`]
/// 6. 非 WebShell 类型返回 PermissionDenied 错误
pub async fn check_agent(
    agent_uuid: String,
    task_token: String,
    task_id: u64,
) -> anyhow::Result<bool> {
    trace!(target: "terminal", agent_uuid = %agent_uuid, task_id = task_id, "checking agent terminal authorization");
    let agent_uuid = Uuid::parse_str(&agent_uuid)
        .map_err(|_| NodegetError::ParseError("Invalid Agent UUID format".to_owned()))?;

    let db = ng_db::get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

    // 按 id + uuid + token + 未完成 四条件联合查询，确认任务归属
    let task_model = task::Entity::find()
        .filter(task::Column::Id.eq(task_id.cast_signed()))
        .filter(task::Column::Uuid.eq(agent_uuid))
        .filter(task::Column::Token.eq(task_token))
        .filter(task::Column::TaskEventResult.is_null())
        .one(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("Database error: {e}")))?;

    let Some(task_model) = task_model else {
        debug!(target: "terminal", agent_uuid = %agent_uuid, task_id = task_id, "未找到匹配的任务记录");
        return Ok(false);
    };

    // 解析任务类型，仅允许 WebShell 类型的任务使用 Terminal
    let task_event_type: TaskEventType = serde_json::from_value(task_model.task_event_type)
        .map_err(|e| {
            NodegetError::SerializationError(format!("Failed to parse task_event_type: {e}"))
        })?;

    if !matches!(task_event_type, TaskEventType::WebShell(_)) {
        warn!(target: "terminal", "权限拒绝: 非 WebShell 任务尝试连接 Terminal, agent_uuid={agent_uuid}, task_id={task_id}");
        return Err(NodegetError::PermissionDenied(
            "Terminal connection is only allowed for WebShell tasks".to_owned(),
        )
        .into());
    }

    Ok(true)
}
