use crate::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Token {
    pub version: u8, // 暂为 1
    pub token_key: String,
    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,
    pub token_limit: Vec<Limit>,
    pub username: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Limit {
    pub scopes: Vec<Scope>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    AgentUuid(uuid::Uuid),
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    StaticMonitoring(StaticMonitoring),
    DynamicMonitoring(DynamicMonitoring),
    Task(Task),
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    Read(StaticDataQueryField),
    Write,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    Read(DynamicDataQueryField),
    Write,
}

// Type 字段名
// 接受 ping / tcp_ping / http_ping / web_shell / execute / ip
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Create(String),
    Read(String),
    Write(String),
    Listen,
}
