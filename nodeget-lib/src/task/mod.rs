use serde::{Deserialize, Serialize};
use toml::Value;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventType {
    Ping(String), // 可能为域名，需解析
    TcpPing(String), // 可能为域名，需解析
    HttpPing(url::Url, String, String), // Url, Method, Body

    WebShell(url::Url), // Websocket URL
    Execute(String), // 命令执行
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskEvent {
    pub task_id: u64,
    pub task_token: String, // 仅用于校验上传者身份，不是鉴权环境之一
    pub task_event_type: TaskEventType,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventResult {
    Ping(f64), // 延迟
    TcpPing(f64), // 延迟
    HttpPing(f64), // 延迟

    WebShell(bool), // Is Connected
    Execute(String), // 命令输出
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskEventResponse {
    pub task_id: u64,
    pub task_token: String,
    pub success: bool,

    pub error_message: Option<Value>,
    pub task_event_result: Option<TaskEventResult>,
}