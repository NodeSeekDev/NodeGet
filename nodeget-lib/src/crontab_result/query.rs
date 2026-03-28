use serde::{Deserialize, Serialize};

// CrontabResult 查询条件枚举，定义 crontab_result 数据查询的各种过滤条件
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResultQueryCondition {
    // 按记录 ID 过滤
    Id(i64),
    // 按 cron_id 过滤
    CronId(i64),
    // 按 cron_name 过滤
    CronName(String),
    // 按时间戳范围过滤（开始时间，结束时间）
    RunTimeFromTo(i64, i64), // start, end (milliseconds)
    // 按时间戳起始点过滤
    RunTimeFrom(i64), // start (milliseconds)
    // 按时间戳结束点过滤
    RunTimeTo(i64), // end (milliseconds)
    // 仅查找成功的记录
    IsSuccess, // 仅查找 success 字段为 true
    // 仅查找失败的记录
    IsFailure, // 仅查找 success 字段为 false
    // 限制返回结果数量
    Limit(u64),
    // 获取最后一条记录
    Last,
}

// CrontabResult 数据查询结构体
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrontabResultDataQuery {
    // 查询条件列表
    pub condition: Vec<CrontabResultQueryCondition>,
}

// CrontabResult 响应项结构体
#[derive(Serialize)]
pub struct CrontabResultResponseItem {
    // 记录 ID
    pub id: i64,
    // Cron ID
    pub cron_id: i64,
    // Cron 名称
    pub cron_name: String,
    pub special_id: Option<i64>,
    // 运行时间戳（毫秒）
    pub run_time: Option<i64>,
    // 是否成功
    pub success: Option<bool>,
    // 消息
    pub message: Option<String>,
}
