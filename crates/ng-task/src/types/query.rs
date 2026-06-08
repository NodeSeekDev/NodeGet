//! 任务查询类型定义：查询条件和响应结构体

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 任务查询条件枚举，各条件之间为 AND 关系
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskQueryCondition {
    /// 按任务 ID 精确过滤
    TaskId(u64),
    /// 按 Agent UUID 精确过滤
    Uuid(uuid::Uuid),
    /// 按时间戳范围过滤（起始毫秒时间戳，结束毫秒时间戳）
    TimestampFromTo(i64, i64),
    /// 按时间戳起始点过滤，大于等于
    TimestampFrom(i64),
    /// 按时间戳结束点过滤，小于等于
    TimestampTo(i64),

    /// 仅查找成功完成的任务（success = true）
    IsSuccess,
    /// 仅查找执行失败的任务（success = false）
    IsFailure,
    /// 仅查找正在运行的任务（success = NULL）
    IsRunning,
    /// 按任务类型过滤，匹配 task_event_type JSON 中包含指定 key 的记录
    Type(String),
    /// 按 cron 来源过滤（由 crontab 创建的任务会写入 cron name）
    CronSource(String),

    /// 限制返回结果数量
    Limit(u64),

    /// 获取最后一条记录（按时间倒序取 1 条）
    Last,
}

/// 任务数据查询结构体，包含查询条件列表
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDataQuery {
    /// 查询条件列表，各条件之间为 AND 关系
    pub condition: Vec<TaskQueryCondition>,
}

/// 任务响应项结构体，用于序列化单条任务查询结果
#[derive(Serialize)]
pub struct TaskResponseItem {
    /// 任务 ID
    pub task_id: i64,
    /// Agent UUID 字符串
    pub uuid: String,
    /// 任务来源的 cron name，非定时任务为 None
    pub cron_source: Option<String>,
    /// 完成时间戳（毫秒），运行中为 None
    pub timestamp: Option<i64>,
    /// 执行是否成功，运行中为 None
    pub success: Option<bool>,
    /// 任务事件类型 JSON
    pub task_event_type: Value,
    /// 任务事件结果 JSON，未完成时为 None
    pub task_event_result: Option<Value>,
    /// 错误消息，成功时为 None
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_query_condition_task_id_serde() {
        let cond = TaskQueryCondition::TaskId(42);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_uuid_serde() {
        let cond = TaskQueryCondition::Uuid(uuid::Uuid::nil());
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_timestamp_range_serde() {
        let cond = TaskQueryCondition::TimestampFromTo(1000, 2000);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_timestamp_from_serde() {
        let cond = TaskQueryCondition::TimestampFrom(1000);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_timestamp_to_serde() {
        let cond = TaskQueryCondition::TimestampTo(2000);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_status_flags_serde() {
        for cond in [
            TaskQueryCondition::IsSuccess,
            TaskQueryCondition::IsFailure,
            TaskQueryCondition::IsRunning,
        ] {
            let json = serde_json::to_string(&cond).unwrap();
            let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
            assert_eq!(cond, parsed);
        }
    }

    #[test]
    fn task_query_condition_type_serde() {
        let cond = TaskQueryCondition::Type("ping".to_owned());
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_cron_source_serde() {
        let cond = TaskQueryCondition::CronSource("my_cron".to_owned());
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_limit_serde() {
        let cond = TaskQueryCondition::Limit(500);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_query_condition_last_serde() {
        let cond = TaskQueryCondition::Last;
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: TaskQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn task_data_query_serde_roundtrip() {
        let query = TaskDataQuery {
            condition: vec![
                TaskQueryCondition::Uuid(uuid::Uuid::nil()),
                TaskQueryCondition::TimestampFromTo(0, 9999),
                TaskQueryCondition::Limit(100),
            ],
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: TaskDataQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query, parsed);
    }

    #[test]
    fn task_data_query_empty_conditions() {
        let query = TaskDataQuery {
            condition: vec![],
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: TaskDataQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query.condition.len(), 0);
        assert_eq!(parsed.condition.len(), 0);
    }

    #[test]
    fn task_query_condition_snake_case_rename() {
        let cond = TaskQueryCondition::IsSuccess;
        let json = serde_json::to_string(&cond).unwrap();
        assert!(json.contains("is_success"));
    }
}
