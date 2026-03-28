pub mod result;

use crate::task::TaskEventType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Cron {
    pub id: i64,
    pub name: String,
    pub enable: bool,
    pub cron_expression: String,
    pub cron_type: CronType,
    pub last_run_time: Option<i64>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronType {
    Agent(Vec<Uuid>, AgentCronType),
    Server(ServerCronType),
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCronType {
    Task(TaskEventType),
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerCronType {
    CleanUpDatabase,
    JsWorker(String, Value) // 脚本名, 传入参数
}
