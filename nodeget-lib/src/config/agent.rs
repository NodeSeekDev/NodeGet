use crate::config::deserialize_uuid_or_auto;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tokio::fs;

// Agent 配置结构体，定义 Agent 的运行参数
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentConfig {
    // 日志级别
    pub log_level: Option<String>,
    // 动态监控数据上报间隔（毫秒），默认 1000（1 秒）
    pub dynamic_report_interval_ms: Option<u64>,
    // 动态监控摘要数据上报间隔（毫秒），默认 1000（1 秒）
    // 必须是 dynamic_report_interval_ms 的因数（即 dynamic_report_interval_ms 是它的整数倍）
    pub dynamic_summary_report_interval_ms: Option<u64>,
    // 静态监控数据上报间隔（毫秒），默认 300000（5 分钟）
    pub static_report_interval_ms: Option<u64>,

    // Agent UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub agent_uuid: uuid::Uuid,

    // WebSocket 连接超时时间（毫秒）
    pub connect_timeout_ms: Option<u64>,

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
    // 服务器 UUID，用于连接时校验服务器身份
    pub server_uuid: String,
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
    // 是否允许 HTTP 请求任务
    pub allow_http_request: Option<bool>, // Dangerous

    // 是否允许获取 IP 地址
    pub allow_ip: Option<bool>,
    // 是否允许获取版本信息
    pub allow_version: Option<bool>,
}

// IP 地址获取服务提供商枚举
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
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

        // 校验 server name 不能重复
        if let Some(servers) = &config.server {
            let mut seen = HashSet::with_capacity(servers.len());
            for server in servers {
                if !seen.insert(&server.name) {
                    return Err(format!("Duplicate server name '{}' in config", server.name).into());
                }
            }
        }

        // 校验 dynamic_report_interval_ms 必须是 dynamic_summary_report_interval_ms 的整数倍
        {
            let dynamic_interval = config.dynamic_report_interval_ms.unwrap_or(1000);
            let summary_interval = config.dynamic_summary_report_interval_ms.unwrap_or(1000);
            if summary_interval == 0 {
                return Err("dynamic_summary_report_interval_ms must be greater than 0".into());
            }
            if dynamic_interval % summary_interval != 0 {
                return Err(format!(
                    "dynamic_report_interval_ms ({dynamic_interval}) must be an integer multiple of dynamic_summary_report_interval_ms ({summary_interval})"
                )
                    .into());
            }
        }

        Ok(config)
    }
}
