use crate::config::deserialize_uuid_or_auto;
use crate::config::replace_auto_gen_uuid;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::fs;

pub const DEFAULT_AGENT_CONFIG_PATH: &str = "config.toml";
pub const DEFAULT_DYNAMIC_REPORT_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_STATIC_REPORT_INTERVAL_MS: u64 = 300_000;
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 1000;
pub const DEFAULT_EXEC_MAX_CHARACTER: usize = 10_000;
pub const DEFAULT_IP_PROVIDER: IpProvider = IpProvider::Cloudflare;
pub const DEFAULT_NTP_SERVER: &str = "pool.ntp.org";

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

    // NTP 服务器地址，默认使用 pool.ntp.org
    pub ntp_server: Option<String>,

    // Disk 选择列表（按 mount_point 匹配），用于 Dynamic Summary 上报
    // 若指定且非空，则仅统计列表中的磁盘；否则回退到默认排除逻辑
    pub dynamic_summary_select_disk: Option<Vec<String>>,

    // 网卡选择列表（按 interface_name 匹配），用于 Dynamic Summary 上报
    // 若指定且非空，则仅统计列表中的网卡；否则回退到默认排除逻辑
    pub dynamic_summary_select_network_interface: Option<Vec<String>>,

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
    // 是否允许 DNS 查询
    pub allow_dns: Option<bool>,
    // 是否允许获取版本信息
    pub allow_version: Option<bool>,
    // 是否允许自更新
    pub allow_self_update: Option<bool>,
    // 是否忽略服务端 TLS 证书校验（默认关闭）
    pub ignore_cert: Option<bool>,

    // 允许的任务类型列表（白名单模式）
    // 若指定，则以此列表为准，忽略所有单独的 allow_* 开关
    // 值为 task_name() 的返回值，如 "ping" / "tcp_ping" / "http_ping" / "dns" / "execute" 等
    pub allow_task_type: Option<Vec<String>>,
}

// IP 地址获取服务提供商枚举
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum IpProvider {
    IpInfo,
    Cloudflare,
}

impl Default for IpProvider {
    fn default() -> Self {
        DEFAULT_IP_PROVIDER
    }
}

impl AgentConfig {
    #[must_use]
    pub fn dynamic_report_interval_ms_or_default(&self) -> u64 {
        self.dynamic_report_interval_ms
            .unwrap_or(DEFAULT_DYNAMIC_REPORT_INTERVAL_MS)
    }

    #[must_use]
    pub fn dynamic_summary_report_interval_ms_or_default(&self) -> u64 {
        self.dynamic_summary_report_interval_ms
            .unwrap_or(DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS)
    }

    #[must_use]
    pub fn static_report_interval_ms_or_default(&self) -> u64 {
        self.static_report_interval_ms
            .unwrap_or(DEFAULT_STATIC_REPORT_INTERVAL_MS)
    }

    #[must_use]
    pub fn connect_timeout_duration(&self) -> Duration {
        Duration::from_millis(
            self.connect_timeout_ms
                .unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS),
        )
    }

    #[must_use]
    pub fn exec_max_character_or_default(&self) -> usize {
        self.exec_max_character
            .unwrap_or(DEFAULT_EXEC_MAX_CHARACTER)
    }

    #[must_use]
    pub fn ip_provider_or_default(&self) -> IpProvider {
        self.ip_provider.unwrap_or(DEFAULT_IP_PROVIDER)
    }

    #[must_use]
    pub fn ntp_server_or_default(&self) -> &str {
        self.ntp_server.as_deref().unwrap_or(DEFAULT_NTP_SERVER)
    }

    /// 从指定路径读取并解析代理配置
    ///
    /// 若配置文件中 `agent_uuid` 为 `"auto_gen"`，则会生成随机 `UUIDv4`
    /// 并直接覆盖原配置文件，后续启动不再触发自动生成。
    ///
    /// # Errors
    ///
    /// 当文件读取失败或TOML解析失败时返回错误
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = fs::read_to_string(path.as_ref()).await?;

        // 检查并替换 auto_gen
        let value: toml::Value = toml::from_str(&content)?;
        let is_auto_gen = value
            .get("agent_uuid")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("auto_gen"));

        let config_content = if is_auto_gen {
            let new_uuid = uuid::Uuid::new_v4().to_string();
            let new_content = replace_auto_gen_uuid(&content, "agent_uuid", &new_uuid);

            // 原子写入：先写 .tmp，成功后 rename，避免崩溃导致配置文件损坏
            let tmp_path = path.as_ref().with_extension("tmp");
            fs::write(&tmp_path, &new_content).await?;
            fs::rename(&tmp_path, path.as_ref()).await?;

            new_content
        } else {
            content
        };

        let config: Self = toml::from_str(&config_content)?;

        if matches!(config.connect_timeout_ms, Some(0)) {
            return Err("connect_timeout_ms must be greater than 0".into());
        }

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
            let dynamic_interval = config.dynamic_report_interval_ms_or_default();
            let summary_interval = config.dynamic_summary_report_interval_ms_or_default();
            if summary_interval == 0 {
                return Err("dynamic_summary_report_interval_ms must be greater than 0".into());
            }
            if !dynamic_interval.is_multiple_of(summary_interval) {
                return Err(format!(
                    "dynamic_report_interval_ms ({dynamic_interval}) must be an integer multiple of dynamic_summary_report_interval_ms ({summary_interval})"
                )
                    .into());
            }
        }

        Ok(config)
    }
}
