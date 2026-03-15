use crate::config::deserialize_uuid_or_auto;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

// Agent 配置结构体，定义 Agent 的运行参数
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentConfig {
    // 日志级别
    pub log_level: Option<String>,
    // 监控数据上报间隔（毫秒）
    pub monitoring_report_interval_ms: Option<u64>,

    // Agent UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub agent_uuid: uuid::Uuid,

    // WebSocket 连接超时时间（毫秒）
    pub connect_timeout_ms: Option<u64>,

    // 执行命令使用的 Shell，Windows 用 cmd，其他系统用 bash 或 sh
    pub exec_shell: Option<String>,
    // 执行命令输出的最大字符数限制
    pub exec_max_character: Option<usize>,

    // 终端 Shell
    pub terminal_shell: Option<String>,

    // IP 地址获取服务提供商
    pub ip_provider: Option<IpProvider>,

    // 服务器列表
    pub server: Option<Vec<Server>>,
}

// 服务器配置结构体，定义 Agent 连接的服务器信息
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Server {
    // 服务器名称
    pub name: String, // Only For Agent
    // 服务器的唯一标识符
    pub uuid: uuid::Uuid,
    // 认证令牌
    pub token: String,
    // WebSocket 连接地址
    pub ws_url: String,

    // 是否允许执行任务
    pub allow_task: Option<bool>,

    // 是否允许 ICMP Ping
    pub allow_icmp_ping: Option<bool>,
    // 是否允许 TCP Ping
    pub allow_tcp_ping: Option<bool>,
    // 是否允许 HTTP Ping
    pub allow_http_ping: Option<bool>,

    // 是否允许 Web Shell
    pub allow_web_shell: Option<bool>,
    // 是否允许阅读配置
    pub allow_read_config: Option<bool>, // Dangerous
    // 是否允许编辑配置
    pub allow_edit_config: Option<bool>, // Dangerous
    // 是否允许执行命令
    pub allow_execute: Option<bool>, // Dangerous

    // 是否允许获取 IP 地址
    pub allow_ip: Option<bool>,
}

// IP 地址获取服务提供商枚举
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum IpProvider {
    IpInfo,
    Cloudflare,
}

impl AgentConfig {
    /// 从指定路径读取并解析代理配置
    ///
    /// # Errors
    ///
    /// 当文件读取失败或TOML解析失败时返回错误
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let file = fs::read_to_string(path).await?;

        let config: Self = toml::from_str(&file)?;

        Ok(config)
    }
}
