use crate::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Token {
    pub version: i32,
    pub token_key: String,
    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,
    pub token_limit: Vec<Limit>,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Limit {
    pub scopes: Vec<Scope>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    AgentUuid(uuid::Uuid),
    KvNamespace(String),
    JsWorker(String),
    StaticBucket(String),
    Db(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    StaticMonitoring(StaticMonitoring),
    DynamicMonitoring(DynamicMonitoring),
    Task(Task),
    Crontab(Crontab),
    CrontabResult(CrontabResult),
    Kv(Kv),
    Terminal(Terminal),
    NodeGet(NodeGet),
    MonitoringUuid(MonitoringUuid),
    JsWorker(JsWorker),
    JsResult(JsResult),
    DynamicMonitoringSummary(DynamicMonitoringSummary),
    StaticBucket(StaticBucket),
    StaticBucketFile(StaticBucketFile),
    Db(Db),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeGet {
    #[deprecated(since = "0.2.13", note = "Use MonitoringUuid::List instead")]
    ListAllAgentUuid,
    GetRtPool,
    #[deprecated(since = "0.2.13", note = "Use MonitoringUuid::Delete instead")]
    DeleteAgentUuid,
    ExecSql,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringUuid {
    List,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    Read(StaticDataQueryField),
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    Read(DynamicDataQueryField),
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoringSummary {
    Read,
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Create(String),
    Read(String),
    Write(String),
    Delete(String),
    Listen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crontab {
    Read,
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResult {
    Read(String),
    Delete(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kv {
    ListAllNamespace,
    ListAllKeys,
    Read(String),
    Write(String),
    Delete(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsWorker {
    ListAllJsWorker,
    Create,
    Read,
    Write,
    Delete,
    RunDefinedJsWorker,
    RunRawJsWorker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsResult {
    Read(String),
    Delete(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticBucket {
    Read,
    Write,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticBucketFile {
    Read,
    Write,
    Delete,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Db {
    List,
    Read,
    Create,
    Update,
    Delete,
    ExecSql,
}
