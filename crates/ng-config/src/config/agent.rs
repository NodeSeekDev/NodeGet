//! Agent 配置文件结构体与解析逻辑。
//!
//! 定义 `AgentConfig`、`Server`（单服务器连接）、`IpProvider` 等类型，
//! 以及默认值常量和配置读取/校验方法。

use crate::config::deserialize_uuid_or_auto;
use crate::config::replace_auto_gen_uuid;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tracing::warn;

/// 默认配置文件路径
pub const DEFAULT_AGENT_CONFIG_PATH: &str = "config.toml";
/// 默认动态监控数据上报间隔（毫秒），1 秒
pub const DEFAULT_DYNAMIC_REPORT_INTERVAL_MS: u64 = 1000;
/// 默认动态摘要上报间隔（毫秒），1 秒
pub const DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS: u64 = 1000;
/// 默认静态监控数据上报间隔（毫秒），5 分钟
pub const DEFAULT_STATIC_REPORT_INTERVAL_MS: u64 = 300_000;
/// 默认 WebSocket 连接超时（毫秒），1 秒
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 1000;
/// 默认执行命令输出最大字符数
pub const DEFAULT_EXEC_MAX_CHARACTER: usize = 10_000;
/// 默认 IP 地址获取服务提供商
pub const DEFAULT_IP_PROVIDER: IpProvider = IpProvider::Cloudflare;
/// 默认 NTP 服务器地址
pub const DEFAULT_NTP_SERVER: &str = "pool.ntp.org";

/// Agent 配置结构体，定义 Agent 的运行参数。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentConfig {
    /// 日志级别
    pub log_level: Option<String>,
    /// 动态监控数据上报间隔（毫秒），默认 1000（1 秒）
    pub dynamic_report_interval_ms: Option<u64>,
    /// 动态监控摘要数据上报间隔（毫秒），默认 1000（1 秒）
    /// 必须是 dynamic_report_interval_ms 的因数（即 dynamic_report_interval_ms 是它的整数倍）
    pub dynamic_summary_report_interval_ms: Option<u64>,
    /// 静态监控数据上报间隔（毫秒），默认 300000（5 分钟）
    pub static_report_interval_ms: Option<u64>,

    /// Agent UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub agent_uuid: uuid::Uuid,

    /// WebSocket 连接超时时间（毫秒）
    pub connect_timeout_ms: Option<u64>,

    /// 执行命令输出的最大字符数限制
    pub exec_max_character: Option<usize>,

    /// 终端 Shell 路径
    pub terminal_shell: Option<String>,

    /// IP 地址获取服务提供商
    pub ip_provider: Option<IpProvider>,

    /// NTP 服务器地址，默认使用 pool.ntp.org
    pub ntp_server: Option<String>,

    /// Disk 选择列表（按 mount_point 匹配），用于 Dynamic Summary 上报。
    /// 若指定且非空，则仅统计列表中的磁盘；否则回退到默认排除逻辑。
    pub dynamic_summary_select_disk: Option<Vec<String>>,

    /// 网卡选择列表（按 interface_name 匹配），用于 Dynamic Summary 上报。
    /// 若指定且非空，则仅统计列表中的网卡；否则回退到默认排除逻辑。
    pub dynamic_summary_select_network_interface: Option<Vec<String>>,

    /// 服务器列表，每个条目对应一个 Server 连接
    pub server: Option<Vec<Server>>,
}

/// 单个服务器连接配置，定义 Agent 连接某个 Server 的信息与权限。
#[derive(Serialize, Deserialize, Clone)]
pub struct Server {
    /// 服务器名称（仅 Agent 端使用，用于标识）
    pub name: String,
    /// 服务器 UUID，用于连接时校验服务器身份
    pub server_uuid: String,
    /// 认证令牌（key:secret 格式）
    pub token: String,
    /// WebSocket 连接地址
    pub ws_url: String,

    /// 是否允许执行任务
    pub allow_task: Option<bool>,

    /// 是否允许 ICMP Ping
    pub allow_icmp_ping: Option<bool>,
    /// 是否允许 TCP Ping
    pub allow_tcp_ping: Option<bool>,
    /// 是否允许 HTTP Ping
    pub allow_http_ping: Option<bool>,

    /// 是否允许 Web Shell
    pub allow_web_shell: Option<bool>,
    /// 是否允许阅读配置（危险操作）
    pub allow_read_config: Option<bool>,
    /// 是否允许编辑配置（危险操作）
    pub allow_edit_config: Option<bool>,
    /// 是否允许执行命令（危险操作）
    pub allow_execute: Option<bool>,
    /// 是否允许 HTTP 请求任务（危险操作）
    pub allow_http_request: Option<bool>,

    /// 是否允许获取 IP 地址
    pub allow_ip: Option<bool>,
    /// 是否允许 DNS 查询
    pub allow_dns: Option<bool>,
    /// 是否允许获取版本信息
    pub allow_version: Option<bool>,
    /// 是否允许自更新
    pub allow_self_update: Option<bool>,
    /// 是否忽略服务端 TLS 证书校验（默认关闭）
    pub ignore_cert: Option<bool>,

    /// 允许的任务类型列表（白名单模式）。
    /// 若指定，则以此列表为准，忽略所有单独的 `allow_*` 开关。
    /// 值为 `task_name()` 的返回值，如 `"ping"` / `"tcp_ping"` / `"http_ping"` / `"dns"` / `"execute"` 等。
    pub allow_task_type: Option<Vec<String>>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("name", &self.name)
            .field("server_uuid", &self.server_uuid)
            // 脱敏处理：不暴露 Token 明文
            .field("token", &"***REDACTED***")
            .field("ws_url", &self.ws_url)
            .field("allow_task", &self.allow_task)
            .field("allow_icmp_ping", &self.allow_icmp_ping)
            .field("allow_tcp_ping", &self.allow_tcp_ping)
            .field("allow_http_ping", &self.allow_http_ping)
            .field("allow_web_shell", &self.allow_web_shell)
            .field("allow_read_config", &self.allow_read_config)
            .field("allow_edit_config", &self.allow_edit_config)
            .field("allow_execute", &self.allow_execute)
            .field("allow_http_request", &self.allow_http_request)
            .field("allow_ip", &self.allow_ip)
            .field("allow_dns", &self.allow_dns)
            .field("allow_version", &self.allow_version)
            .field("allow_self_update", &self.allow_self_update)
            .field("ignore_cert", &self.ignore_cert)
            .field("allow_task_type", &self.allow_task_type)
            .finish()
    }
}

/// IP 地址获取服务提供商枚举。
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum IpProvider {
    /// ipinfo.io 服务
    IpInfo,
    /// Cloudflare 1.1.1.1 服务
    Cloudflare,
}

impl Default for IpProvider {
    fn default() -> Self {
        DEFAULT_IP_PROVIDER
    }
}

impl AgentConfig {
    /// 获取动态上报间隔，未配置时使用默认值（1 秒）。
    #[must_use]
    pub fn dynamic_report_interval_ms_or_default(&self) -> u64 {
        self.dynamic_report_interval_ms
            .unwrap_or(DEFAULT_DYNAMIC_REPORT_INTERVAL_MS)
    }

    /// 获取动态摘要上报间隔，未配置时使用默认值（1 秒）。
    #[must_use]
    pub fn dynamic_summary_report_interval_ms_or_default(&self) -> u64 {
        self.dynamic_summary_report_interval_ms
            .unwrap_or(DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS)
    }

    /// 获取静态上报间隔，未配置时使用默认值（5 分钟）。
    #[must_use]
    pub fn static_report_interval_ms_or_default(&self) -> u64 {
        self.static_report_interval_ms
            .unwrap_or(DEFAULT_STATIC_REPORT_INTERVAL_MS)
    }

    /// 获取连接超时 Duration，未配置时使用默认值（1 秒）。
    #[must_use]
    pub fn connect_timeout_duration(&self) -> Duration {
        Duration::from_millis(
            self.connect_timeout_ms
                .unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS),
        )
    }

    /// 获取执行命令最大字符数，未配置时使用默认值（10000）。
    #[must_use]
    pub fn exec_max_character_or_default(&self) -> usize {
        self.exec_max_character
            .unwrap_or(DEFAULT_EXEC_MAX_CHARACTER)
    }

    /// 获取 IP 服务提供商，未配置时使用默认值（Cloudflare）。
    #[must_use]
    pub fn ip_provider_or_default(&self) -> IpProvider {
        self.ip_provider.unwrap_or(DEFAULT_IP_PROVIDER)
    }

    /// 获取 NTP 服务器地址，未配置时使用默认值（pool.ntp.org）。
    #[must_use]
    pub fn ntp_server_or_default(&self) -> &str {
        self.ntp_server.as_deref().unwrap_or(DEFAULT_NTP_SERVER)
    }

    /// 从指定路径读取并解析 Agent 配置。
    ///
    /// 若配置文件中 `agent_uuid` 为 `"auto_gen"`，则会生成随机 UUIDv4
    /// 并直接覆盖原配置文件，后续启动不再触发自动生成。
    ///
    /// - `path` — 配置文件路径
    /// - 返回解析后的 `AgentConfig`
    ///
    /// 内部步骤：
    /// 1. 读取配置文件内容
    /// 2. 检查 `agent_uuid` 是否为 `auto_gen`，若是则生成 UUID 并原子写回
    /// 3. 解析 TOML 为 `AgentConfig`
    /// 4. 校验 `connect_timeout_ms` 不为零
    /// 5. 校验 server name 不重复
    /// 6. 校验 `dynamic_report_interval_ms` 是 `dynamic_summary_report_interval_ms` 的整数倍
    ///
    /// # Errors
    ///
    /// 当文件读取失败、TOML 解析失败、或校验不通过时返回错误
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 1. 读取配置文件内容
        let content = fs::read_to_string(path.as_ref()).await?;

        // 2. 检查并替换 auto_gen
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

        // 3. 解析 TOML 为 AgentConfig
        let config: Self = toml::from_str(&config_content)?;

        // 4. 校验 connect_timeout_ms 不为零
        if matches!(config.connect_timeout_ms, Some(0)) {
            warn!(target: "config", "配置验证失败: connect_timeout_ms 不能为 0");
            return Err("connect_timeout_ms must be greater than 0".into());
        }

        // 5. 校验 server name 不能重复
        if let Some(servers) = &config.server {
            let mut seen = HashSet::with_capacity(servers.len());
            for server in servers {
                if !seen.insert(&server.name) {
                    warn!(target: "config", "配置验证失败: 重复的服务器名称 '{}'", server.name);
                    return Err(format!("Duplicate server name '{}' in config", server.name).into());
                }
            }
        }

        // 6. 校验 dynamic_report_interval_ms 必须是 dynamic_summary_report_interval_ms 的整数倍
        {
            let dynamic_interval = config.dynamic_report_interval_ms_or_default();
            let summary_interval = config.dynamic_summary_report_interval_ms_or_default();
            if summary_interval == 0 {
                warn!(target: "config", "配置验证失败: dynamic_summary_report_interval_ms 不能为 0");
                return Err("dynamic_summary_report_interval_ms must be greater than 0".into());
            }
            if !dynamic_interval.is_multiple_of(summary_interval) {
                warn!(target: "config", "配置验证失败: dynamic_report_interval_ms ({dynamic_interval}) 不是 dynamic_summary_report_interval_ms ({summary_interval}) 的整数倍");
                return Err(format!(
                    "dynamic_report_interval_ms ({dynamic_interval}) must be an integer multiple of dynamic_summary_report_interval_ms ({summary_interval})"
                )
                    .into());
            }
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_config_minimal_toml() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.agent_uuid,
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
        assert!(config.log_level.is_none());
        assert!(config.dynamic_report_interval_ms.is_none());
        assert!(config.static_report_interval_ms.is_none());
        assert!(config.connect_timeout_ms.is_none());
        assert!(config.exec_max_character.is_none());
        assert!(config.terminal_shell.is_none());
        assert!(config.ip_provider.is_none());
        assert!(config.ntp_server.is_none());
        assert!(config.server.is_none());
    }

    #[test]
    fn agent_config_full_toml() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
log_level = "debug"
dynamic_report_interval_ms = 2000
dynamic_summary_report_interval_ms = 1000
static_report_interval_ms = 600000
connect_timeout_ms = 3000
exec_max_character = 5000
terminal_shell = "/bin/zsh"
ip_provider = "ipinfo"
ntp_server = "time.google.com"
dynamic_summary_select_disk = ["/", "/data"]
dynamic_summary_select_network_interface = ["eth0"]

[[server]]
name = "prod"
server_uuid = "660e8400-e29b-41d4-a716-446655440001"
token = "key:secret"
ws_url = "ws://1.2.3.4:3000/nodeget/rpc"
allow_task = true
allow_icmp_ping = true
allow_tcp_ping = true
allow_http_ping = true
allow_web_shell = true
allow_read_config = true
allow_edit_config = false
allow_execute = true
allow_http_request = true
allow_ip = true
allow_dns = true
allow_version = true
allow_self_update = true
ignore_cert = false

[[server]]
name = "dev"
server_uuid = "660e8400-e29b-41d4-a716-446655440002"
token = "key2:secret2"
ws_url = "wss://dev.example.com/nodeget/rpc"
allow_task_type = ["ping", "tcp_ping", "version"]
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log_level, Some("debug".to_owned()));
        assert_eq!(config.dynamic_report_interval_ms, Some(2000));
        assert_eq!(config.dynamic_summary_report_interval_ms, Some(1000));
        assert_eq!(config.static_report_interval_ms, Some(600000));
        assert_eq!(config.connect_timeout_ms, Some(3000));
        assert_eq!(config.exec_max_character, Some(5000));
        assert_eq!(config.terminal_shell, Some("/bin/zsh".to_owned()));
        assert!(matches!(config.ip_provider, Some(IpProvider::IpInfo)));
        assert_eq!(config.ntp_server, Some("time.google.com".to_owned()));
        assert_eq!(
            config.dynamic_summary_select_disk,
            Some(vec!["/".to_owned(), "/data".to_owned()])
        );
        assert_eq!(
            config.dynamic_summary_select_network_interface,
            Some(vec!["eth0".to_owned()])
        );

        let servers = config.server.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "prod");
        assert_eq!(servers[0].allow_task, Some(true));
        assert_eq!(servers[0].allow_icmp_ping, Some(true));
        assert_eq!(servers[0].allow_edit_config, Some(false));
        assert_eq!(servers[0].allow_task_type, None);
        assert_eq!(servers[1].name, "dev");
        assert_eq!(
            servers[1].allow_task_type,
            Some(vec!["ping".to_owned(), "tcp_ping".to_owned(), "version".to_owned()])
        );
    }

    #[test]
    fn ip_provider_deserialize() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
ip_provider = "cloudflare"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.ip_provider, Some(IpProvider::Cloudflare)));

        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
ip_provider = "ipinfo"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.ip_provider, Some(IpProvider::IpInfo)));
    }

    #[test]
    fn ip_provider_default_is_cloudflare() {
        assert!(matches!(IpProvider::default(), IpProvider::Cloudflare));
    }

    #[test]
    fn agent_config_default_intervals() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.dynamic_report_interval_ms_or_default(), DEFAULT_DYNAMIC_REPORT_INTERVAL_MS);
        assert_eq!(config.dynamic_summary_report_interval_ms_or_default(), DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS);
        assert_eq!(config.static_report_interval_ms_or_default(), DEFAULT_STATIC_REPORT_INTERVAL_MS);
    }

    #[test]
    fn agent_config_connect_timeout_duration() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
connect_timeout_ms = 5000
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.connect_timeout_duration(), Duration::from_millis(5000));
    }

    #[test]
    fn agent_config_connect_timeout_default() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.connect_timeout_duration(), Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS));
    }

    #[test]
    fn agent_config_exec_max_character_default() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.exec_max_character_or_default(), DEFAULT_EXEC_MAX_CHARACTER);
    }

    #[test]
    fn agent_config_ntp_server_default() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ntp_server_or_default(), DEFAULT_NTP_SERVER);
    }

    #[test]
    fn agent_config_ip_provider_default() {
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(config.ip_provider_or_default(), IpProvider::Cloudflare));
    }

    #[test]
    fn server_debug_redacts_token() {
        let server = Server {
            name: "test".to_owned(),
            server_uuid: "uuid".to_owned(),
            token: "secret-key:super-secret".to_owned(),
            ws_url: "ws://localhost:3000".to_owned(),
            allow_task: Some(true),
            allow_icmp_ping: None,
            allow_tcp_ping: None,
            allow_http_ping: None,
            allow_web_shell: None,
            allow_read_config: None,
            allow_edit_config: None,
            allow_execute: None,
            allow_http_request: None,
            allow_ip: None,
            allow_dns: None,
            allow_version: None,
            allow_self_update: None,
            ignore_cert: None,
            allow_task_type: None,
        };
        let debug_output = format!("{server:?}");
        assert!(debug_output.contains("***REDACTED***"));
        assert!(!debug_output.contains("super-secret"));
        assert!(debug_output.contains("test"));
    }

    #[test]
    fn agent_config_default_constants() {
        assert_eq!(DEFAULT_DYNAMIC_REPORT_INTERVAL_MS, 1000);
        assert_eq!(DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS, 1000);
        assert_eq!(DEFAULT_STATIC_REPORT_INTERVAL_MS, 300_000);
        assert_eq!(DEFAULT_CONNECT_TIMEOUT_MS, 1000);
        assert_eq!(DEFAULT_EXEC_MAX_CHARACTER, 10_000);
        assert_eq!(DEFAULT_NTP_SERVER, "pool.ntp.org");
    }

    #[test]
    fn agent_config_roundtrip_json() {
        // Use serde_json for roundtrip since toml::to_string needs the "display" feature
        let toml_str = r#"
agent_uuid = "550e8400-e29b-41d4-a716-446655440000"
log_level = "info"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.agent_uuid, deserialized.agent_uuid);
        assert_eq!(config.log_level, deserialized.log_level);
    }
}
