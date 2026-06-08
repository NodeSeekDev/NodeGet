//! Server 配置文件结构体与解析逻辑。
//!
//! 定义 `ServerConfig`、`DatabaseConfig`、`LoggingConfig`、`MonitoringBufferConfig` 等类型，
//! 以及配置读取与 auto_gen UUID 替换方法。

use crate::config::deserialize_uuid_or_auto;
use crate::config::replace_auto_gen_uuid;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// 服务器配置结构体，定义 Server 的运行参数。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    /// Server UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub server_uuid: uuid::Uuid,

    /// WebSocket 监听地址（如 `0.0.0.0:3000`）
    pub ws_listener: String,

    /// JSON-RPC 最大并发连接数，默认 100
    pub jsonrpc_max_connections: Option<u32>,

    /// 是否启用 Unix Socket（仅非 Windows 平台）
    pub enable_unix_socket: Option<bool>,

    /// Unix Socket 路径（仅非 Windows 平台，默认 `/var/lib/nodeget.sock`）
    pub unix_socket_path: Option<String>,

    /// 日志配置（可选，不填则使用默认值）
    pub logging: Option<LoggingConfig>,

    /// 数据库连接配置
    pub database: DatabaseConfig,

    /// 监控数据缓冲写入配置（可选）
    pub monitoring_buffer: Option<MonitoringBufferConfig>,

    /// JSON-RPC 最大请求体大小（字节），默认 10485760（10MB）
    pub max_request_body_size: Option<u32>,

    /// JSON-RPC 最大响应体大小（字节），默认 104857600（100MB）
    pub max_response_body_size: Option<u32>,

    /// TLS 证书文件路径（PEM 格式），必须与 `tls_key` 同时指定才启用 TLS
    pub tls_cert: Option<String>,

    /// TLS 私钥文件路径（PEM 格式），必须与 `tls_cert` 同时指定才启用 TLS
    pub tls_key: Option<String>,

    /// 静态文件服务根目录，默认 `./static/`
    pub static_path: Option<String>,

    /// 本地 SQLite 数据库存放目录，默认 `./db/`
    pub db_path: Option<String>,
}

/// 监控数据缓冲写入配置。
///
/// 控制 `report_static`、`report_dynamic`、`report_dynamic_summary` 三个写入接口
/// 的批量插入行为。启用后，数据先缓存在内存中，按间隔批量写入数据库。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MonitoringBufferConfig {
    /// 刷新间隔（毫秒），默认 500
    pub flush_interval_ms: Option<u64>,
    /// 单次最大批量大小，默认 1000
    pub max_batch_size: Option<usize>,
    /// Channel 容量，默认 10000
    pub channel_capacity: Option<usize>,
}

/// 日志配置。
///
/// `log_filter` / `json_log_filter` 的语法与 `RUST_LOG` 环境变量一致，
/// 例如 `"info,kv=debug,monitoring=trace,db=warn"`。
///
/// 可用的 target：
///   `server`、`rpc`、`db`、`kv`、`monitoring`、`task`、`token`、
///   `js_worker`、`js_result`、`crontab`、`crontab_result`、
///   `js_runtime`、`terminal`
///
/// 虚拟 target `db` 会自动展开为
/// `sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`。
///
/// 如果设置了 `RUST_LOG` 环境变量，它会覆盖 `log_filter`。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoggingConfig {
    /// 控制台日志过滤器，语法同 `RUST_LOG`，默认 `"info"`
    pub log_filter: Option<String>,

    /// JSON 日志输出文件路径（可选，不设置则不输出 JSON 日志）
    pub json_log_file: Option<String>,

    /// JSON 日志过滤器，语法同 `RUST_LOG`（可选，默认与 `log_filter` 相同）
    pub json_log_filter: Option<String>,

    /// 内存日志缓冲区容量（条数），默认 500，设为 0 表示禁用内存日志
    pub memory_log_capacity: Option<usize>,

    /// 内存日志过滤器，语法同 `RUST_LOG`（可选，默认与 `log_filter` 相同）
    /// 通过 `nodeget-server_log` RPC 方法可查询缓冲区内容
    pub memory_log_filter: Option<String>,
}

/// 数据库配置结构体，定义数据库连接参数。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatabaseConfig {
    /// 数据库连接 URL
    pub database_url: String,
    /// 数据库连接超时时间（毫秒）
    pub connect_timeout_ms: Option<u64>,
    /// 获取连接超时时间（毫秒）
    pub acquire_timeout_ms: Option<u64>,
    /// 连接空闲超时时间（毫秒）
    pub idle_timeout_ms: Option<u64>,
    /// 连接最大生存时间（毫秒）
    pub max_lifetime_ms: Option<u64>,
    /// 最大连接数
    pub max_connections: Option<u32>,
}

impl ServerConfig {
    /// 从指定路径读取并解析服务器配置。
    ///
    /// 若配置文件中 `server_uuid` 为 `"auto_gen"`，则会生成随机 UUIDv4
    /// 并直接覆盖原配置文件，后续启动不再触发自动生成。
    ///
    /// - `path` — 配置文件路径
    /// - 返回解析后的 `ServerConfig`
    ///
    /// 内部步骤：
    /// 1. 读取配置文件内容
    /// 2. 检查 `server_uuid` 是否为 `auto_gen`，若是则生成 UUID 并原子写回
    /// 3. 解析 TOML 为 `ServerConfig`
    ///
    /// # Errors
    ///
    /// 当文件读取失败或 TOML 解析失败时返回错误
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 1. 读取配置文件内容
        let content = fs::read_to_string(path.as_ref()).await?;

        // 2. 检查并替换 auto_gen
        let value: toml::Value = toml::from_str(&content)?;
        let is_auto_gen = value
            .get("server_uuid")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("auto_gen"));

        if is_auto_gen {
            let new_uuid = uuid::Uuid::new_v4().to_string();
            let new_content = replace_auto_gen_uuid(&content, "server_uuid", &new_uuid);

            // 原子写入：先写 .tmp，成功后 rename，避免崩溃导致配置文件损坏
            let tmp_path = path.as_ref().with_extension("tmp");
            fs::write(&tmp_path, &new_content).await?;
            fs::rename(&tmp_path, path.as_ref()).await?;

            // 3. 解析替换后的配置
            let config: Self = toml::from_str(&new_content)?;
            return Ok(config);
        }

        // 3. 解析原始配置（无需替换）
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_minimal_toml() {
        let toml_str = r#"
server_uuid = "550e8400-e29b-41d4-a716-446655440000"
ws_listener = "0.0.0.0:3000"

[database]
database_url = "sqlite://./db/nodeget.db"
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.server_uuid,
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
        assert_eq!(config.ws_listener, "0.0.0.0:3000");
        assert_eq!(config.database.database_url, "sqlite://./db/nodeget.db");
        assert!(config.jsonrpc_max_connections.is_none());
        assert!(config.enable_unix_socket.is_none());
        assert!(config.logging.is_none());
        assert!(config.monitoring_buffer.is_none());
        assert!(config.max_request_body_size.is_none());
        assert!(config.max_response_body_size.is_none());
        assert!(config.tls_cert.is_none());
        assert!(config.tls_key.is_none());
        assert!(config.static_path.is_none());
        assert!(config.db_path.is_none());
    }

    #[test]
    fn server_config_full_toml() {
        let toml_str = r#"
server_uuid = "550e8400-e29b-41d4-a716-446655440000"
ws_listener = "0.0.0.0:3000"
jsonrpc_max_connections = 200
enable_unix_socket = true
unix_socket_path = "/tmp/nodeget.sock"
max_request_body_size = 5242880
max_response_body_size = 52428800
tls_cert = "/etc/tls/cert.pem"
tls_key = "/etc/tls/key.pem"
static_path = "./files/"
db_path = "./data/"

[database]
database_url = "postgres://user:pass@localhost/nodeget"
connect_timeout_ms = 5000
acquire_timeout_ms = 10000
idle_timeout_ms = 600000
max_lifetime_ms = 1800000
max_connections = 20

[logging]
log_filter = "info,db=warn"
json_log_file = "/var/log/nodeget.json"
json_log_filter = "debug"
memory_log_capacity = 1000
memory_log_filter = "trace"

[monitoring_buffer]
flush_interval_ms = 1000
max_batch_size = 500
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.jsonrpc_max_connections, Some(200));
        assert_eq!(config.enable_unix_socket, Some(true));
        assert_eq!(config.unix_socket_path, Some("/tmp/nodeget.sock".to_owned()));
        assert_eq!(config.max_request_body_size, Some(5242880));
        assert_eq!(config.max_response_body_size, Some(52428800));
        assert_eq!(config.tls_cert, Some("/etc/tls/cert.pem".to_owned()));
        assert_eq!(config.tls_key, Some("/etc/tls/key.pem".to_owned()));
        assert_eq!(config.static_path, Some("./files/".to_owned()));
        assert_eq!(config.db_path, Some("./data/".to_owned()));

        let db = &config.database;
        assert_eq!(db.database_url, "postgres://user:pass@localhost/nodeget");
        assert_eq!(db.connect_timeout_ms, Some(5000));
        assert_eq!(db.acquire_timeout_ms, Some(10000));
        assert_eq!(db.idle_timeout_ms, Some(600000));
        assert_eq!(db.max_lifetime_ms, Some(1800000));
        assert_eq!(db.max_connections, Some(20));

        let logging = config.logging.unwrap();
        assert_eq!(logging.log_filter, Some("info,db=warn".to_owned()));
        assert_eq!(logging.json_log_file, Some("/var/log/nodeget.json".to_owned()));
        assert_eq!(logging.json_log_filter, Some("debug".to_owned()));
        assert_eq!(logging.memory_log_capacity, Some(1000));
        assert_eq!(logging.memory_log_filter, Some("trace".to_owned()));

        let buffer = config.monitoring_buffer.unwrap();
        assert_eq!(buffer.flush_interval_ms, Some(1000));
        assert_eq!(buffer.max_batch_size, Some(500));
    }

    #[test]
    fn monitoring_buffer_config_defaults() {
        let toml_str = r#"
[monitoring_buffer]
"#;
        let config: MonitoringBufferConfig = toml::from_str(toml_str).unwrap();
        assert!(config.flush_interval_ms.is_none());
        assert!(config.max_batch_size.is_none());
    }

    #[test]
    fn database_config_required_fields() {
        let toml_str = r#"
database_url = "sqlite://./db/nodeget.db"
"#;
        let config: DatabaseConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database_url, "sqlite://./db/nodeget.db");
        assert!(config.connect_timeout_ms.is_none());
        assert!(config.max_connections.is_none());
    }

    #[test]
    fn logging_config_partial() {
        let toml_str = r#"
log_filter = "debug"
"#;
        let config: LoggingConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.log_filter, Some("debug".to_owned()));
        assert!(config.json_log_file.is_none());
        assert!(config.json_log_filter.is_none());
        assert!(config.memory_log_capacity.is_none());
        assert!(config.memory_log_filter.is_none());
    }

    #[test]
    fn server_config_missing_database_fails() {
        let toml_str = r#"
server_uuid = "550e8400-e29b-41d4-a716-446655440000"
ws_listener = "0.0.0.0:3000"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn server_config_missing_ws_listener_fails() {
        let toml_str = r#"
server_uuid = "550e8400-e29b-41d4-a716-446655440000"

[database]
database_url = "sqlite://./db/nodeget.db"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn server_config_roundtrip_json() {
        // Use serde_json for roundtrip since toml::to_string needs the "display" feature
        let toml_str = r#"
server_uuid = "550e8400-e29b-41d4-a716-446655440000"
ws_listener = "0.0.0.0:3000"

[database]
database_url = "sqlite://./db/nodeget.db"
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.server_uuid, deserialized.server_uuid);
        assert_eq!(config.ws_listener, deserialized.ws_listener);
        assert_eq!(config.database.database_url, deserialized.database.database_url);
    }
}
