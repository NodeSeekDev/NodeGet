use serde::{Deserialize, Serialize};
use serde_json::Value;

// 任务查询条件枚举，定义任务数据查询的各种过滤条件
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskQueryCondition {
    // 按任务 ID 过滤
    TaskId(u64),
    // 按 UUID 过滤
    Uuid(uuid::Uuid),
    // 按时间戳范围过滤（开始时间，结束时间）
    TimestampFromTo(i64, i64), // start, end
    // 按时间戳起始点过滤
    TimestampFrom(i64), // start,
    // 按时间戳结束点过滤
    TimestampTo(i64), // end

    // 仅查找成功完成的任务
    IsSuccess, // 仅查找 success 字段为 true
    // 仅查找执行失败的任务
    IsFailure, // 仅查找 success 字段为 false
    // 仅查找正在运行的任务
    IsRunning, // 仅查找 success 字段为空
    // 按任务类型过滤，task_event_type 中有字段为 `String` 的行
    Type(String),
    // 按 cron 来源过滤（由 crontab 创建的任务会写入 cron name）
    CronSource(String),

    // 限制返回结果数量
    Limit(u64), // limit

    // 获取最后一条记录
    Last,
}

// 任务数据查询结构体
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDataQuery {
    // 查询条件列表
    pub condition: Vec<TaskQueryCondition>,
}

// 任务响应项结构体
#[derive(Serialize)]
pub struct TaskResponseItem {
    // 任务 ID
    pub task_id: i64,
    // UUID
    pub uuid: String,
    // 任务来源的 cron name，非定时任务为 None
    pub cron_source: Option<String>,
    // 时间戳，可选参数
    pub timestamp: Option<i64>,
    // 执行是否成功，可选参数
    pub success: Option<bool>,
    // 任务事件类型
    pub task_event_type: Value,
    // 任务事件结果，可选参数
    pub task_event_result: Option<Value>,
    // 错误消息，可选参数
    pub error_message: Option<String>,
}
