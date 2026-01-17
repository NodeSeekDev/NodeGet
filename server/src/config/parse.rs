
use std::path::Path;
use tokio::fs;
use crate::config::ServerConfig;

impl ServerConfig {
    pub async fn get_and_parse_config(
        path: impl AsRef<Path>,
    ) -> Result<ServerConfig, Box<dyn std::error::Error>> {
        let file = fs::read_to_string(path).await?;

        let config: ServerConfig = toml::from_str(&file)?;

        Ok(config)
    }
}
