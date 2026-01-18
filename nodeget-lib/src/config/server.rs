use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::config::deserialize_uuid_or_auto;
use tokio::fs;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    pub log_level: String,
    #[serde(deserialize_with = "deserialize_uuid_or_auto")]
    pub server_uuid: uuid::Uuid,
    pub ws_listener: String,
    pub database: DatabaseConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DatabaseConfig {
    pub database_url: String,
    pub sqlx_log_level: Option<String>,
    pub connect_timeout_ms: Option<u64>,
    pub acquire_timeout_ms: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub max_connections: Option<u32>,
}

impl ServerConfig {
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<ServerConfig, Box<dyn std::error::Error>> {
        let file = fs::read_to_string(path).await?;

        let config: ServerConfig = toml::from_str(&file)?;

        Ok(config)
    }
}
