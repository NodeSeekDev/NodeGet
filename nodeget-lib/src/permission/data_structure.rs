use crate::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};
use serde::{Deserialize, Serialize};

// 令牌结构体，定义权限令牌的完整信息
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Token {
    // 令牌版本号，目前为 1
    pub version: i32, // 暂为 1
    // 令牌密钥，用于标识令牌的主要键
    pub token_key: String,
    // 令牌生效时间戳（毫秒），可选参数
    pub timestamp_from: Option<i64>,
    // 令牌过期时间戳（毫秒），可选参数
    pub timestamp_to: Option<i64>,
    // 令牌权限限制列表
    pub token_limit: Vec<Limit>,
    // 用户名，可选参数
    pub username: Option<String>,
}

// 权限限制结构体，定义特定作用域下的权限集合
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Limit {
    // 作用域列表
    pub scopes: Vec<Scope>,
    // 权限列表
    pub permissions: Vec<Permission>,
}

// 作用域枚举，定义权限的作用范围
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    // 全局作用域，适用于所有地点
    Global,
    // 特定 Agent 作用域，通过 UUID 指定
    AgentUuid(uuid::Uuid),
    // KvNamespace 作用域，通过名称指定
    KvNamespace(String),
}

// 权限枚举，定义不同类型的操作权限
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    // 静态监控权限
    StaticMonitoring(StaticMonitoring),
    // 动态监控权限
    DynamicMonitoring(DynamicMonitoring),
    // 任务权限
    Task(Task),
    // Crontab 权限
    Crontab(Crontab),

    // Kv 权限
    Kv(Kv),

    // Terminal 权限
    Terminal(Terminal),

    // CrontabResult 权限
    CrontabResult(CrontabResult),

    // NodeGet 权限
    NodeGet(NodeGet),
}

// 静态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    // 读取权限，指定可读取的字段类型
    Read(StaticDataQueryField),
    // 写入权限
    Write,
    // 删除权限
    Delete,
}

// 动态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    // 读取权限，指定可读取的字段类型
    Read(DynamicDataQueryField),
    // 写入权限
    Write,
    // 删除权限
    Delete,
}

// 任务权限枚举
// Type 字段名
// 接受 ping / tcp_ping / http_ping / web_shell / execute / ip
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    // 创建权限，指定任务类型
    Create(String),
    // 读取权限，指定任务类型
    Read(String),
    // 写入权限，指定任务类型
    Write(String),
    // 监听权限
    Listen,
}

// Crontab 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crontab {
    Read,
    Write,
    Delete,
}

// CrontabResult 权限枚举
// 注意：该权限仅在 Global Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResult {
    // 读取权限，指定可读取的 cron_name
    Read(String),
    // 删除权限，指定可删除的 cron_name
    Delete(String),
}

// Kv 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kv {
    ListAllNamespace,
    ListAllKeys,
    Read(String),
    Write(String),
    Delete(String),
}

// Terminal 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    Connect,
}

// NodeGet 权限枚举
// 在 Global Scope 下可列出系统内全部 Agent UUID
// 在 AgentUuid Scope 下可列出对应范围内的 Agent UUID（仍需方法层校验）
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeGet {
    // 列出所有 Agent Uuid
    ListAllAgentUuid,
}
