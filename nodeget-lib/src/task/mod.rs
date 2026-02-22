// 任务查询模块
#[cfg(feature = "for-server")]
pub mod query;

use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

// 任务事件类型枚举，定义各种可执行的任务类型
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventType {
    // Ping 任务，可能为域名需要解析
    Ping(String), // 可能为域名，需解析
    // TCP Ping 任务，可能为域名需要解析
    TcpPing(String), // 可能为域名，需解析
    // HTTP Ping 任务，使用 URL
    HttpPing(url::Url), // Url

    // Web Shell 任务，使用 WebSocket URL
    WebShell(url::Url), // Websocket URL
    // 命令执行任务
    Execute(String), // 命令执行

    // IP 获取任务
    Ip,
}

impl TaskEventType {
    /// 获取任务类型的名称标识符
    #[must_use]
    pub const fn task_name(&self) -> &'static str {
        match self {
            Self::Ping(_) => "ping",
            Self::TcpPing(_) => "tcp_ping",
            Self::HttpPing(_) => "http_ping",
            Self::WebShell(_) => "web_shell",
            Self::Execute(_) => "execute",
            Self::Ip => "ip",
        }
    }

    /// 从延迟创建对应的结果类型
    /// 用于 Ping/TcpPing/HttpPing 任务
    ///
    /// # Panics
    /// 当任务类型不是 Ping/TcpPing/HttpPing 时会 panic
    #[must_use]
    pub fn result_from_duration(&self, duration: Duration) -> TaskEventResult {
        let millis = duration.as_millis_f64();
        match self {
            Self::Ping(_) => TaskEventResult::Ping(millis),
            Self::TcpPing(_) => TaskEventResult::TcpPing(millis),
            Self::HttpPing(_) => TaskEventResult::HttpPing(millis),
            _ => panic!("result_from_duration only valid for ping tasks"),
        }
    }

    /// 检查任务类型是否为延迟测试类任务
    #[must_use]
    pub const fn is_ping_task(&self) -> bool {
        matches!(self, Self::Ping(_) | Self::TcpPing(_) | Self::HttpPing(_))
    }

    /// 获取任务的权限检查字段名
    /// 用于 Agent 配置中的权限字段匹配
    #[must_use]
    pub const fn permission_field(&self) -> &'static str {
        match self {
            Self::Ping(_) => "allow_icmp_ping",
            Self::TcpPing(_) => "allow_tcp_ping",
            Self::HttpPing(_) => "allow_http_ping",
            Self::WebShell(_) => "allow_web_shell",
            Self::Execute(_) => "allow_execute",
            Self::Ip => "allow_ip",
        }
    }
}

// 任务事件结构体，定义单个任务的详细信息
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    // 任务 ID
    pub task_id: u64,
    // 任务令牌，仅用于校验上传者身份，不是鉴权环境之一
    pub task_token: String, // 仅用于校验上传者身份，不是鉴权环境之一
    // 任务事件类型
    pub task_event_type: TaskEventType,
}

// 任务事件结果枚举，定义任务执行后的返回结果
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventResult {
    // Ping 任务结果，返回延迟（毫秒）
    Ping(f64), // 延迟
    // TCP Ping 任务结果，返回延迟（毫秒）
    TcpPing(f64), // 延迟
    // HTTP Ping 任务结果，返回延迟（毫秒）
    HttpPing(f64), // 延迟

    // Web Shell 任务结果，返回是否连接成功
    WebShell(bool), // Is Connected
    // 命令执行任务结果，返回命令输出
    Execute(String), // 命令输出

    // IP 获取任务结果，返回 IPv4 和 IPv6 地址
    Ip(Option<Ipv4Addr>, Option<Ipv6Addr>), // V4 V6 IP
}

impl TaskEventResult {
    /// 获取结果类型对应的任务名称
    #[must_use]
    pub const fn task_name(&self) -> &'static str {
        match self {
            Self::Ping(_) => "ping",
            Self::TcpPing(_) => "tcp_ping",
            Self::HttpPing(_) => "http_ping",
            Self::WebShell(_) => "web_shell",
            Self::Execute(_) => "execute",
            Self::Ip(_, _) => "ip",
        }
    }

    /// 从延迟创建结果（用于 Ping/TcpPing/HttpPing）
    #[must_use]
    pub const fn from_duration(task_type: &TaskEventType, duration: Duration) -> Option<Self> {
        match task_type {
            TaskEventType::Ping(_) => Some(Self::Ping(duration.as_millis_f64())),
            TaskEventType::TcpPing(_) => Some(Self::TcpPing(duration.as_millis_f64())),
            TaskEventType::HttpPing(_) => Some(Self::HttpPing(duration.as_millis_f64())),
            _ => None,
        }
    }
}

// 任务事件响应结构体，定义任务执行后的响应信息
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct TaskEventResponse {
    // 任务 ID
    pub task_id: u64,
    // Agent 的 UUID
    pub agent_uuid: uuid::Uuid,
    // 任务令牌
    pub task_token: String,
    // 时间戳
    pub timestamp: u64,

    // 执行是否成功
    pub success: bool,

    // 错误消息，可选参数
    pub error_message: Option<String>,
    // 任务事件结果，可选参数
    pub task_event_result: Option<TaskEventResult>,
}
