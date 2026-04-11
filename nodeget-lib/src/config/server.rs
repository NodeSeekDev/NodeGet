use crate::config::deserialize_uuid_or_auto;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

// 服务器配置结构体
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    // Server UUID，默认自动生成
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub server_uuid: uuid::Uuid,

    // WebSocket 监听地址
    pub ws_listener: String,

    // JSON-RPC 最大并发连接数，默认 100
    pub jsonrpc_max_connections: Option<u32>,

    // 是否启用 Unix Socket（仅非 Windows 平台）
    pub enable_unix_socket: Option<bool>,

    // Unix Socket 路径（仅非 Windows 平台，默认 /var/lib/nodeget.sock）
    pub unix_socket_path: Option<String>,

    // 日志配置（可选，不填则使用默认值）
    pub logging: Option<LoggingConfig>,

    // 数据库配置
    pub database: DatabaseConfig,
}

/// 日志配置
///
/// `log_filter` / `json_log_filter` 的语法与 `RUST_LOG` 环境变量一致，
/// 例如 `"info,rpc=debug,db=warn"`。
///
/// 虚拟 target `db` 会自动展开为
/// `sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`。
///
/// 如果设置了 `RUST_LOG` 环境变量，它会覆盖 `log_filter`。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoggingConfig {
    /// 控制台日志过滤器，语法同 RUST_LOG，默认 "info"
    pub log_filter: Option<String>,

    /// JSON 日志输出文件路径（可选，不设置则不输出 JSON 日志）
    pub json_log_file: Option<String>,

    /// JSON 日志过滤器，语法同 RUST_LOG（可选，默认与 log_filter 相同）
    pub json_log_filter: Option<String>,

    /// 内存日志缓冲区容量（条数），默认 500
    pub memory_log_capacity: Option<usize>,

    /// 内存日志过滤器，语法同 RUST_LOG（可选，默认与 log_filter 相同）
    /// 通过 nodeget-server_log RPC 方法可查询缓冲区内容
    pub memory_log_filter: Option<String>,
}

// 数据库配置结构体，定义数据库连接参数
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatabaseConfig {
    // 数据库连接 URL
    pub database_url: String,
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
