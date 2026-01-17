mod parse;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    pub log_level: String,

    pub server_uuid: String,
    pub database_url: String,

    pub ws_listener: String,
}