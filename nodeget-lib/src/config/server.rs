use crate::config::deserialize_uuid_or_auto;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

// 服务器配置结构体
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    // 日志级别
    pub log_level: String,
    // Server UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub server_uuid: uuid::Uuid,

    // WebSocket 监听地址
    pub ws_listener: String,

    // JSON-RPC 最大并发连接数，默认 100
    pub jsonrpc_max_connections: Option<u32>,

    // JSON-RPC 耗时日志级别，默认 trace
    pub jsonrpc_timing_log_level: Option<String>,

    // 是否启用 Unix Socket（仅非 Windows 平台）
    pub enable_unix_socket: Option<bool>,

    // Unix Socket 路径（仅非 Windows 平台，默认 /var/lib/nodeget.sock）
    pub unix_socket_path: Option<String>,

    // 数据库配置
    pub database: DatabaseConfig,
}

// 数据库配置结构体，定义数据库连接参数
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatabaseConfig {
    // 数据库连接 URL
    pub database_url: String,
    // SQLx 日志级别
    pub sqlx_log_level: Option<String>,
    // 数据库连接超时时间（毫秒）
    pub connect_timeout_ms: Option<u64>,
    // 获取连接超时时间（毫秒）
    pub acquire_timeout_ms: Option<u64>,
    // 连接空闲超时时间（毫秒）
    pub idle_timeout_ms: Option<u64>,
    // 连接最大生存时间（毫秒）
    pub max_lifetime_ms: Option<u64>,
    // 最大连接数
    pub max_connections: Option<u32>,
}

impl ServerConfig {
    /// 从指定路径读取并解析服务器配置
    ///
    /// # Errors
    ///
    /// 当文件读取失败或TOML解析失败时返回错误
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let file = fs::read_to_string(path).await?;

        let config: Self = toml::from_str(&file)?;

        Ok(config)
    }
}
