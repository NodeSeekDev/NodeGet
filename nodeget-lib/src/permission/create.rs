use crate::permission::data_structure::Limit;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenCreationRequest {
    pub username: Option<String>,
    pub password: Option<String>,

    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,

    pub version: Option<u8>,

    pub token_limit: Vec<Limit>,
}
