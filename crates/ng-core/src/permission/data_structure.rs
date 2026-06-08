//! RBAC 权限模型核心数据结构
//!
//! 定义 Token、Limit、Scope、Permission 等类型，构成 NodeGet 的
//! 基于作用域的权限控制体系。Token 携带多条 Limit，每条 Limit
//! 由作用域（Scope）+ 权限（Permission）组合约束。

use crate::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};
use serde::{Deserialize, Serialize};

/// 已验证的 Token 信息，由鉴权层返回。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Token {
    /// Token 版本号
    pub version: i32,
    /// Token 的 key 标识
    pub token_key: String,
    /// 有效期起始（Unix 毫秒，None 表示无下界）
    pub timestamp_from: Option<i64>,
    /// 有效期截止（Unix 毫秒，None 表示无上界）
    pub timestamp_to: Option<i64>,
    /// 权限限制列表
    pub token_limit: Vec<Limit>,
    /// 关联的用户名
    pub username: Option<String>,
}

/// 单条权限限制：作用域 + 允许的操作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Limit {
    /// 此条限制适用的作用域列表
    pub scopes: Vec<Scope>,
    /// 此条限制允许的权限列表
    pub permissions: Vec<Permission>,
}

/// 权限作用域：限定 Token 可操作的资源范围。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// 全局作用域，不限定具体资源
    Global,
    /// 特定 Agent UUID
    AgentUuid(uuid::Uuid),
    /// 特定 KV 命名空间
    KvNamespace(String),
    /// 特定 JS Worker
    JsWorker(String),
    /// 特定静态文件桶
    StaticBucket(String),
    /// 特定数据库连接
    Db(String),
}

/// 权限枚举：每个变体对应一个业务模块的细粒度操作权限。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// 静态监控数据权限
    StaticMonitoring(StaticMonitoring),
    /// 动态监控数据权限
    DynamicMonitoring(DynamicMonitoring),
    /// 任务调度权限
    Task(Task),
    /// 定时任务权限
    Crontab(Crontab),
    /// 定时任务执行结果权限
    CrontabResult(CrontabResult),
    /// KV 存储权限
    Kv(Kv),
    /// 终端连接权限
    Terminal(Terminal),
    /// NodeGet 服务级权限
    NodeGet(NodeGet),
    /// 监控 UUID 管理权限
    MonitoringUuid(MonitoringUuid),
    /// JS Worker 权限
    JsWorker(JsWorker),
    /// JS 执行结果权限
    JsResult(JsResult),
    /// 动态监控汇总权限
    DynamicMonitoringSummary(DynamicMonitoringSummary),
    /// 静态文件桶权限
    StaticBucket(StaticBucket),
    /// 静态文件操作权限
    StaticBucketFile(StaticBucketFile),
    /// 数据库连接管理权限
    Db(Db),
}

/// NodeGet 服务级操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeGet {
    /// 列出所有 Agent UUID（已废弃，使用 MonitoringUuid::List）
    #[deprecated(since = "0.2.13", note = "Use MonitoringUuid::List instead")]
    ListAllAgentUuid,
    /// 获取 JS 运行时池状态
    GetRtPool,
    /// 删除 Agent UUID（已废弃，使用 MonitoringUuid::Delete）
    #[deprecated(since = "0.2.13", note = "Use MonitoringUuid::Delete instead")]
    DeleteAgentUuid,
    /// 执行原生 SQL
    ExecSql,
}

/// 监控 UUID 管理权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringUuid {
    /// 列出 UUID
    List,
    /// 删除 UUID
    Delete,
}

/// 静态监控数据操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    /// 读取指定字段（附带字段粒度控制）
    Read(StaticDataQueryField),
    /// 上报写入
    Write,
    /// 删除记录
    Delete,
}

/// 动态监控数据操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    /// 读取指定字段（附带字段粒度控制）
    Read(DynamicDataQueryField),
    /// 上报写入
    Write,
    /// 删除记录
    Delete,
}

/// 动态监控汇总数据操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoringSummary {
    /// 读取汇总
    Read,
    /// 上报汇总
    Write,
    /// 删除汇总
    Delete,
}

/// 任务调度操作权限，关联任务类型名
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// 创建指定类型的任务
    Create(String),
    /// 读取指定类型的任务
    Read(String),
    /// 修改指定类型的任务
    Write(String),
    /// 删除指定类型的任务
    Delete(String),
    /// 监听任务事件
    Listen,
}

/// 定时任务操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crontab {
    /// 读取定时任务配置
    Read,
    /// 创建或修改定时任务
    Write,
    /// 删除定时任务
    Delete,
}

/// 定时任务执行结果操作权限，关联 Cron 名称
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResult {
    /// 读取指定 Cron 的执行结果
    Read(String),
    /// 删除指定 Cron 的执行结果
    Delete(String),
}

/// KV 存储操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kv {
    /// 列出所有命名空间
    ListAllNamespace,
    /// 列出所有键
    ListAllKeys,
    /// 读取指定命名空间的值
    Read(String),
    /// 写入指定命名空间的值
    Write(String),
    /// 删除指定命名空间的键
    Delete(String),
}

/// 终端连接权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    /// 建立 WebSocket 终端连接
    Connect,
}

/// JS Worker 操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsWorker {
    /// 列出所有 JS Worker
    ListAllJsWorker,
    /// 创建 JS Worker
    Create,
    /// 读取 JS Worker 配置
    Read,
    /// 修改 JS Worker 配置
    Write,
    /// 删除 JS Worker
    Delete,
    /// 执行已注册的 JS Worker
    RunDefinedJsWorker,
    /// 执行内联 JS 代码
    RunRawJsWorker,
}

/// JS 执行结果操作权限，关联 Worker 名称
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsResult {
    /// 读取指定 Worker 的执行结果
    Read(String),
    /// 删除指定 Worker 的执行结果
    Delete(String),
}

/// 静态文件桶操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticBucket {
    /// 读取桶信息
    Read,
    /// 创建或修改桶
    Write,
    /// 删除桶
    Delete,
}

/// 静态文件操作权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticBucketFile {
    /// 读取文件
    Read,
    /// 上传文件
    Write,
    /// 删除文件
    Delete,
    /// 列出文件目录
    List,
}

/// 数据库连接管理权限
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Db {
    /// 列出所有数据库连接
    List,
    /// 读取数据库连接信息
    Read,
    /// 创建数据库连接
    Create,
    /// 更新数据库连接
    Update,
    /// 删除数据库连接
    Delete,
    /// 执行原生 SQL
    ExecSql,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ── Scope ───────────────────────────────────────────────────────

    #[test]
    fn scope_global() {
        let s = Scope::Global;
        assert_eq!(format!("{s:?}"), "Global");
        assert_eq!(s, Scope::Global);
    }

    #[test]
    fn scope_agent_uuid() {
        let id = uuid::Uuid::new_v4();
        let s = Scope::AgentUuid(id);
        assert_eq!(s, Scope::AgentUuid(id));
        assert_ne!(s, Scope::Global);
    }

    #[test]
    fn scope_kv_namespace() {
        let s = Scope::KvNamespace("ns".into());
        assert_eq!(s, Scope::KvNamespace("ns".into()));
    }

    #[test]
    fn scope_js_worker() {
        let s = Scope::JsWorker("w".into());
        assert_eq!(s, Scope::JsWorker("w".into()));
    }

    #[test]
    fn scope_static_bucket() {
        let s = Scope::StaticBucket("b".into());
        assert_eq!(s, Scope::StaticBucket("b".into()));
    }

    #[test]
    fn scope_db() {
        let s = Scope::Db("mydb".into());
        assert_eq!(s, Scope::Db("mydb".into()));
    }

    #[test]
    fn scope_clone() {
        let s = Scope::KvNamespace("ns".into());
        assert_eq!(s.clone(), s);
    }

    #[test]
    fn scope_hash_eq() {
        let mut set = HashSet::new();
        set.insert(Scope::Global);
        assert!(set.contains(&Scope::Global));
        assert!(!set.contains(&Scope::KvNamespace("x".into())));
    }

    #[test]
    fn scope_serde_round_trip() {
        let s = Scope::AgentUuid(uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let json = serde_json::to_string(&s).unwrap();
        let de: Scope = serde_json::from_str(&json).unwrap();
        assert_eq!(s, de);
    }

    // ── Permission ──────────────────────────────────────────────────

    #[test]
    fn permission_static_monitoring_read() {
        let p = Permission::StaticMonitoring(StaticMonitoring::Read(
            crate::monitoring::query::StaticDataQueryField::Cpu,
        ));
        assert_eq!(p.clone(), p);
    }

    #[test]
    fn permission_static_monitoring_write() {
        let p = Permission::StaticMonitoring(StaticMonitoring::Write);
        assert_eq!(p, Permission::StaticMonitoring(StaticMonitoring::Write));
    }

    #[test]
    fn permission_static_monitoring_delete() {
        let p = Permission::StaticMonitoring(StaticMonitoring::Delete);
        assert!(format!("{p:?}").contains("Delete"));
    }

    #[test]
    fn permission_dynamic_monitoring() {
        let p = Permission::DynamicMonitoring(DynamicMonitoring::Read(
            crate::monitoring::query::DynamicDataQueryField::Ram,
        ));
        assert_eq!(p.clone(), p);
    }

    #[test]
    fn permission_dynamic_monitoring_write_delete() {
        assert_eq!(Permission::DynamicMonitoring(DynamicMonitoring::Write), Permission::DynamicMonitoring(DynamicMonitoring::Write));
        assert_eq!(Permission::DynamicMonitoring(DynamicMonitoring::Delete), Permission::DynamicMonitoring(DynamicMonitoring::Delete));
    }

    #[test]
    fn permission_dynamic_monitoring_summary() {
        let p = Permission::DynamicMonitoringSummary(DynamicMonitoringSummary::Read);
        assert_eq!(p.clone(), p);
        assert_eq!(Permission::DynamicMonitoringSummary(DynamicMonitoringSummary::Write), Permission::DynamicMonitoringSummary(DynamicMonitoringSummary::Write));
        assert_eq!(Permission::DynamicMonitoringSummary(DynamicMonitoringSummary::Delete), Permission::DynamicMonitoringSummary(DynamicMonitoringSummary::Delete));
    }

    #[test]
    fn permission_task_variants() {
        assert_eq!(Permission::Task(Task::Create("t".into())), Permission::Task(Task::Create("t".into())));
        assert_eq!(Permission::Task(Task::Read("t".into())), Permission::Task(Task::Read("t".into())));
        assert_eq!(Permission::Task(Task::Write("t".into())), Permission::Task(Task::Write("t".into())));
        assert_eq!(Permission::Task(Task::Delete("t".into())), Permission::Task(Task::Delete("t".into())));
        assert_eq!(Permission::Task(Task::Listen), Permission::Task(Task::Listen));
    }

    #[test]
    fn permission_crontab() {
        assert_eq!(Permission::Crontab(Crontab::Read), Permission::Crontab(Crontab::Read));
        assert_eq!(Permission::Crontab(Crontab::Write), Permission::Crontab(Crontab::Write));
        assert_eq!(Permission::Crontab(Crontab::Delete), Permission::Crontab(Crontab::Delete));
    }

    #[test]
    fn permission_crontab_result() {
        assert_eq!(Permission::CrontabResult(CrontabResult::Read("c".into())), Permission::CrontabResult(CrontabResult::Read("c".into())));
        assert_eq!(Permission::CrontabResult(CrontabResult::Delete("c".into())), Permission::CrontabResult(CrontabResult::Delete("c".into())));
    }

    #[test]
    fn permission_kv() {
        assert_eq!(Permission::Kv(Kv::ListAllNamespace), Permission::Kv(Kv::ListAllNamespace));
        assert_eq!(Permission::Kv(Kv::ListAllKeys), Permission::Kv(Kv::ListAllKeys));
        assert_eq!(Permission::Kv(Kv::Read("ns".into())), Permission::Kv(Kv::Read("ns".into())));
        assert_eq!(Permission::Kv(Kv::Write("ns".into())), Permission::Kv(Kv::Write("ns".into())));
        assert_eq!(Permission::Kv(Kv::Delete("ns".into())), Permission::Kv(Kv::Delete("ns".into())));
    }

    #[test]
    fn permission_terminal() {
        assert_eq!(Permission::Terminal(Terminal::Connect), Permission::Terminal(Terminal::Connect));
    }

    #[test]
    fn permission_nodeget() {
        assert_eq!(Permission::NodeGet(NodeGet::GetRtPool), Permission::NodeGet(NodeGet::GetRtPool));
        assert_eq!(Permission::NodeGet(NodeGet::ExecSql), Permission::NodeGet(NodeGet::ExecSql));
    }

    #[test]
    fn permission_nodeget_deprecated_variants() {
        // Deprecated variants still construct correctly
        #[expect(deprecated)]
        let _p1 = Permission::NodeGet(NodeGet::ListAllAgentUuid);
        #[expect(deprecated)]
        let _p2 = Permission::NodeGet(NodeGet::DeleteAgentUuid);
    }

    #[test]
    fn permission_monitoring_uuid() {
        assert_eq!(Permission::MonitoringUuid(MonitoringUuid::List), Permission::MonitoringUuid(MonitoringUuid::List));
        assert_eq!(Permission::MonitoringUuid(MonitoringUuid::Delete), Permission::MonitoringUuid(MonitoringUuid::Delete));
    }

    #[test]
    fn permission_js_worker() {
        assert_eq!(Permission::JsWorker(JsWorker::ListAllJsWorker), Permission::JsWorker(JsWorker::ListAllJsWorker));
        assert_eq!(Permission::JsWorker(JsWorker::Create), Permission::JsWorker(JsWorker::Create));
        assert_eq!(Permission::JsWorker(JsWorker::Read), Permission::JsWorker(JsWorker::Read));
        assert_eq!(Permission::JsWorker(JsWorker::Write), Permission::JsWorker(JsWorker::Write));
        assert_eq!(Permission::JsWorker(JsWorker::Delete), Permission::JsWorker(JsWorker::Delete));
        assert_eq!(Permission::JsWorker(JsWorker::RunDefinedJsWorker), Permission::JsWorker(JsWorker::RunDefinedJsWorker));
        assert_eq!(Permission::JsWorker(JsWorker::RunRawJsWorker), Permission::JsWorker(JsWorker::RunRawJsWorker));
    }

    #[test]
    fn permission_js_result() {
        assert_eq!(Permission::JsResult(JsResult::Read("w".into())), Permission::JsResult(JsResult::Read("w".into())));
        assert_eq!(Permission::JsResult(JsResult::Delete("w".into())), Permission::JsResult(JsResult::Delete("w".into())));
    }

    #[test]
    fn permission_static_bucket() {
        assert_eq!(Permission::StaticBucket(StaticBucket::Read), Permission::StaticBucket(StaticBucket::Read));
        assert_eq!(Permission::StaticBucket(StaticBucket::Write), Permission::StaticBucket(StaticBucket::Write));
        assert_eq!(Permission::StaticBucket(StaticBucket::Delete), Permission::StaticBucket(StaticBucket::Delete));
    }

    #[test]
    fn permission_static_bucket_file() {
        assert_eq!(Permission::StaticBucketFile(StaticBucketFile::Read), Permission::StaticBucketFile(StaticBucketFile::Read));
        assert_eq!(Permission::StaticBucketFile(StaticBucketFile::Write), Permission::StaticBucketFile(StaticBucketFile::Write));
        assert_eq!(Permission::StaticBucketFile(StaticBucketFile::Delete), Permission::StaticBucketFile(StaticBucketFile::Delete));
        assert_eq!(Permission::StaticBucketFile(StaticBucketFile::List), Permission::StaticBucketFile(StaticBucketFile::List));
    }

    #[test]
    fn permission_db() {
        assert_eq!(Permission::Db(Db::List), Permission::Db(Db::List));
        assert_eq!(Permission::Db(Db::Read), Permission::Db(Db::Read));
        assert_eq!(Permission::Db(Db::Create), Permission::Db(Db::Create));
        assert_eq!(Permission::Db(Db::Update), Permission::Db(Db::Update));
        assert_eq!(Permission::Db(Db::Delete), Permission::Db(Db::Delete));
        assert_eq!(Permission::Db(Db::ExecSql), Permission::Db(Db::ExecSql));
    }

    #[test]
    fn permission_serde_round_trip() {
        let p = Permission::Kv(Kv::Read("myns".into()));
        let json = serde_json::to_string(&p).unwrap();
        let de: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(p, de);
    }

    #[test]
    fn permission_different_variants_not_equal() {
        let a = Permission::Crontab(Crontab::Read);
        let b = Permission::Crontab(Crontab::Write);
        assert_ne!(a, b);
    }

    // ── Limit ───────────────────────────────────────────────────────

    #[test]
    fn limit_construction_and_fields() {
        let l = Limit {
            scopes: vec![Scope::Global, Scope::KvNamespace("ns".into())],
            permissions: vec![Permission::Kv(Kv::Read("ns".into()))],
        };
        assert_eq!(l.scopes.len(), 2);
        assert_eq!(l.permissions.len(), 1);
    }

    #[test]
    fn limit_clone_eq() {
        let l = Limit {
            scopes: vec![Scope::Global],
            permissions: vec![Permission::Terminal(Terminal::Connect)],
        };
        assert_eq!(l.clone(), l);
    }

    #[test]
    fn limit_empty_scopes_and_permissions() {
        let l = Limit {
            scopes: vec![],
            permissions: vec![],
        };
        assert!(l.scopes.is_empty());
        assert!(l.permissions.is_empty());
    }

    #[test]
    fn limit_serde_round_trip() {
        let l = Limit {
            scopes: vec![Scope::Db("mydb".into())],
            permissions: vec![Permission::Db(Db::ExecSql)],
        };
        let json = serde_json::to_string(&l).unwrap();
        let de: Limit = serde_json::from_str(&json).unwrap();
        assert_eq!(l, de);
    }

    // ── Token ───────────────────────────────────────────────────────

    #[test]
    fn token_construction_and_fields() {
        let t = Token {
            version: 1,
            token_key: "abc".into(),
            timestamp_from: Some(1000),
            timestamp_to: Some(2000),
            token_limit: vec![Limit {
                scopes: vec![Scope::Global],
                permissions: vec![Permission::NodeGet(NodeGet::ExecSql)],
            }],
            username: Some("admin".into()),
        };
        assert_eq!(t.version, 1);
        assert_eq!(t.token_key, "abc");
        assert_eq!(t.timestamp_from, Some(1000));
        assert_eq!(t.timestamp_to, Some(2000));
        assert_eq!(t.token_limit.len(), 1);
        assert_eq!(t.username, Some("admin".into()));
    }

    #[test]
    fn token_optional_fields_none() {
        let t = Token {
            version: 2,
            token_key: "k".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        assert!(t.timestamp_from.is_none());
        assert!(t.timestamp_to.is_none());
        assert!(t.token_limit.is_empty());
        assert!(t.username.is_none());
    }

    #[test]
    fn token_eq() {
        let t1 = Token {
            version: 1,
            token_key: "k".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        let t2 = Token {
            version: 1,
            token_key: "k".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        assert_eq!(t1, t2);
    }

    #[test]
    fn token_different_not_eq() {
        let t1 = Token {
            version: 1,
            token_key: "k1".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        let t2 = Token {
            version: 2,
            token_key: "k2".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        assert_ne!(t1, t2);
    }

    #[test]
    fn token_serde_round_trip() {
        let t = Token {
            version: 3,
            token_key: "serde_key".into(),
            timestamp_from: Some(100),
            timestamp_to: Some(200),
            token_limit: vec![Limit {
                scopes: vec![Scope::JsWorker("w".into())],
                permissions: vec![Permission::JsWorker(JsWorker::Read)],
            }],
            username: Some("user".into()),
        };
        let json = serde_json::to_string(&t).unwrap();
        let de: Token = serde_json::from_str(&json).unwrap();
        assert_eq!(t, de);
    }

    #[test]
    fn token_debug() {
        let t = Token {
            version: 1,
            token_key: "k".into(),
            timestamp_from: None,
            timestamp_to: None,
            token_limit: vec![],
            username: None,
        };
        let debug = format!("{t:?}");
        assert!(debug.contains("Token"));
        assert!(debug.contains("token_key"));
    }
}
