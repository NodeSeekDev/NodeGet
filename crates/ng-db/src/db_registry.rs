//! 用户数据库注册表与连接池管理
//!
//! 核心职责：
//! - 管理用户通过 `db.create` RPC 创建的 `SQLite` 数据库连接池
//! - 后台定时清理过期连接（基于 `max_lifetime_ms`）
//! - 启动时从 `db_registry` 表种子恢复已有连接
//! - 提供 SQL 执行结果转换工具（`row_to_json`、`json_to_sea_value`、`is_read_query`）
//!
//! 协作关系：
//! - `db` 命名空间 RPC 通过 `DbRegistryManager::global()` 访问连接池
//! - 服务端启动时调用 `DbRegistryManager::init`，关闭时调用 `shutdown`
//! - 主库全局单例 `get_db()` 用于读写 `db_registry` 表

use crate::entity::db_registry as dbreg_entity;
use crate::get_db;
use anyhow::Context;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// 全局 `DbRegistryManager` 单例，服务端启动时通过 `init` 写入
static MGR: std::sync::OnceLock<Arc<DbRegistryManager>> = std::sync::OnceLock::new();

/// 带最后使用时间追踪的数据库连接，用于过期清理判定
struct TrackedConnection {
    /// `SeaORM` 数据库连接实例
    conn: DatabaseConnection,
    /// 最后一次被访问的 UNIX 时间戳（毫秒），由 `get_conn` / `create_conn` 更新
    last_used_ms: AtomicU64,
}

/// 用户数据库连接池管理器
///
/// 维护所有用户通过 RPC 创建的 `SQLite` 数据库连接，提供 CRUD 和过期清理。
/// 全局唯一实例，通过 `init` / `global` 访问。
pub struct DbRegistryManager {
    /// `SQLite` 数据库文件存放目录路径
    db_path: String,
    /// 数据库名称 → 连接的映射，RwLock 保护并发读写
    pools: RwLock<HashMap<String, Arc<TrackedConnection>>>,
    /// 清理循环取消标志，`shutdown` 时设为 true
    cancelled: AtomicBool,
    /// 通知清理循环立即退出的信号
    cancel_notify: Notify,
    /// 清理循环的 `JoinHandle`，`shutdown` 时用于等待退出
    cleanup_handle: Mutex<Option<JoinHandle<()>>>,
}

/// 获取当前 UNIX 时间戳（毫秒），用于 `last_used_ms` 追踪
fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

impl DbRegistryManager {
    /// 初始化全局 `DbRegistryManager` 单例并启动后台清理循环
    ///
    /// - `db_path` — `SQLite` 数据库文件存放目录
    /// - 返回值：全局单例的 Arc 引用
    ///
    /// 内部步骤：
    /// 1. `call_once` 保证仅初始化一次
    /// 2. 创建 Manager 实例
    /// 3. 在 spawn 的异步任务中从 `db_registry` 表种子恢复已有连接
    /// 4. 启动定时清理循环
    ///
    /// # Panics
    ///
    /// 若 `Mutex` 被 poison（仅当其他持有者在持锁期间 panic 时可能发生），或
    /// `OnceLock` 内部 `expect` 失败时会 panic
    pub async fn init(db_path: String) -> Arc<Self> {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mgr_inner = Arc::new(Self {
                db_path,
                pools: RwLock::new(HashMap::new()),
                cancelled: AtomicBool::new(false),
                cancel_notify: Notify::new(),
                cleanup_handle: Mutex::new(None),
            });
            let mgr_clone = Arc::clone(&mgr_inner);
            let handle = tokio::spawn(async move {
                if let Err(e) = mgr_clone.seed_from_dbreg().await {
                    warn!(target: "db", error = %e, "Failed to seed db_registry from persisted state");
                }
                mgr_clone.start_cleanup_loop().await;
            });
            *mgr_inner.cleanup_handle.lock().unwrap() = Some(handle);
            let _ = MGR.set(mgr_inner);
        });
        Arc::clone(MGR.get().expect("DbRegistryManager not initialized"))
    }

    /// 获取全局单例引用，初始化前调用会 panic
    ///
    /// # Panics
    ///
    /// 若 `DbRegistryManager` 尚未初始化（即 `init` 未被调用）时会 panic
    pub fn global() -> &'static Arc<Self> {
        MGR.get().expect("DbRegistryManager not initialized")
    }

    /// 从主库 `db_registry` 表恢复已有连接到内存池
    ///
    /// - 读取所有 `db_registry` 条目
    /// - 校验数据库名称合法性（与 `validate_db_name` 规则一致）
    /// - 为每个合法条目创建 `SQLite` 连接并启用 WAL 等优化
    /// - 跳过名称非法的条目并输出警告
    async fn seed_from_dbreg(&self) -> anyhow::Result<()> {
        let main_db = get_main_db()?;
        let entries = dbreg_entity::Entity::find().all(main_db).await?;
        let db_base = self.db_path.trim_end_matches('/');
        // 锁外构建连接（磁盘 I/O），避免写锁阻塞所有池访问
        let mut built: Vec<(String, Arc<TrackedConnection>)> = Vec::with_capacity(entries.len());
        for entry in entries {
            let name = &entry.name;
            let valid = !name.is_empty()
                && name.len() <= 128
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
                && name != "."
                && name != "..";
            if !valid {
                warn!(target: "db", name = %name, "Skipping db_registry entry with invalid name during seed");
                continue;
            }
            let db_url = format!("sqlite://{db_base}/{name}.db?mode=rwc");
            match Database::connect(&db_url).await {
                Ok(conn) => {
                    if conn.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
                        let _ = conn.execute_unprepared("PRAGMA journal_mode=WAL;").await;
                        let _ = conn.execute_unprepared("PRAGMA synchronous=NORMAL;").await;
                        let _ = conn.execute_unprepared("PRAGMA busy_timeout = 5000;").await;
                        let _ = conn.execute_unprepared("PRAGMA foreign_keys = ON;").await;
                    }
                    built.push((
                        name.clone(),
                        Arc::new(TrackedConnection {
                            conn,
                            last_used_ms: AtomicU64::new(now_ms_u64()),
                        }),
                    ));
                    info!(target: "db", name = %name, "Restored database connection from registry");
                }
                Err(e) => {
                    error!(target: "db", name = %name, error = %e, "Failed to restore database connection");
                }
            }
        }
        // 锁内仅做快速 HashMap 插入
        let mut pools = self.pools.write().await;
        for (name, tracked) in built {
            pools.insert(name, tracked);
        }
        Ok(())
    }

    /// 后台定时清理循环，每分钟检查一次过期连接
    ///
    /// 退出条件：`cancelled` 标志设为 true 或收到 `cancel_notify` 信号
    async fn start_cleanup_loop(&self) {
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                info!(target: "db", "DbRegistry cleanup loop stopped");
                break;
            }
            tokio::select! {
                () = self.cancel_notify.notified() => {
                    info!(target: "db", "DbRegistry cleanup loop stopped");
                    break;
                }
                () = tokio::time::sleep(std::time::Duration::from_mins(1)) => {
                    if let Err(e) = self.cleanup_expired().await {
                        warn!(target: "db", error = %e, "DbRegistry cleanup failed, will retry next cycle");
                    }
                }
            }
        }
    }

    /// 清理所有超过 `max_lifetime_ms` 的过期连接
    ///
    /// 内部步骤：
    /// 1. 在读锁下收集所有连接的名称和最后使用时间
    /// 2. 释放锁后逐一查询 `db_registry` 表获取 `max_lifetime_ms`
    /// 3. 判定过期后调用 `remove_conn` 移除连接、删除文件
    async fn cleanup_expired(&self) -> anyhow::Result<()> {
        let main_db = get_main_db()?;
        // 先在读锁下收集候选条目，释放锁后再做 DB 查询和过期判定，避免长时间持锁
        let candidates: Vec<(String, u64)> = {
            let pools = self.pools.read().await;
            pools
                .iter()
                .map(|(name, tracked)| (name.clone(), tracked.last_used_ms.load(Ordering::Relaxed)))
                .collect()
        };
        // 不持锁进行 DB 查询和过期判定
        let mut to_remove = Vec::new();
        for (name, last_used) in candidates {
            match dbreg_entity::Entity::find()
                .filter(dbreg_entity::Column::Name.eq(&name))
                .one(main_db)
                .await
            {
                Ok(Some(m)) => {
                    if let Some(lifetime_ms) = m.max_lifetime_ms {
                        let elapsed_ms = now_ms_u64().saturating_sub(last_used) as i64;
                        if elapsed_ms >= lifetime_ms {
                            to_remove.push(name);
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(target: "db", name = %name, error = %e, "Failed to query db_registry entry, skipping");
                }
            }
        }
        for name in to_remove {
            if let Err(e) = self.remove_conn(&name).await {
                warn!(target: "db", name = %name, error = %e, "Failed to remove expired connection");
            } else {
                info!(target: "db", name = %name, "Expired connection cleaned up");
            }
        }
        Ok(())
    }

    /// 根据数据库名称获取其 `SQLite` 文件路径
    ///
    /// - `name` — 数据库名称
    /// - 返回值：格式为 `{db_path}/{name}.db` 的绝对路径字符串
    pub fn get_db_path(&self, name: &str) -> String {
        format!("{}/{}.db", self.db_path.trim_end_matches('/'), name)
    }

    /// 轻量级连接存在性检查，避免克隆 `DatabaseConnection`
    ///
    /// - `name` — 数据库名称
    /// - 返回值：连接池中是否存在该名称的连接
    pub async fn has_conn(&self, name: &str) -> bool {
        let pools = self.pools.read().await;
        pools.contains_key(name)
    }

    /// 获取指定数据库的连接并刷新最后使用时间
    ///
    /// - `name` — 数据库名称
    /// - 返回值：连接存在返回 `Some(DatabaseConnection)`，否则 `None`
    pub async fn get_conn(&self, name: &str) -> Option<DatabaseConnection> {
        let pools = self.pools.read().await;
        pools.get(name).map(|tracked| {
            tracked.last_used_ms.store(now_ms_u64(), Ordering::Relaxed);
            tracked.conn.clone()
        })
    }

    /// 创建新的数据库连接并注册到连接池和 `db_registry` 表
    ///
    /// - `name` — 数据库名称
    /// - `max_lifetime_ms` — 连接最大生命周期（毫秒），`None` 表示永不过期
    /// - 返回值：新创建的 `DatabaseConnection`
    ///
    /// 内部步骤：
    /// 1. 构造 `SQLite` URL 并连接（自动启用 WAL 等优化）
    /// 2. 若 `db_registry` 中无记录则插入新行，否则递增 `db_connections` 计数
    /// 3. 将连接加入内存池并记录最后使用时间
    ///
    /// # Errors
    ///
    /// 当 `SQLite` 连接失败或 `db_registry` 表操作失败时返回错误
    ///
    /// # Panics
    ///
    /// 若 `existing` 在 `else` 分支中为 `None`（逻辑上不应发生）时会 panic
    pub async fn create_conn(
        &self,
        name: &str,
        max_lifetime_ms: Option<i64>,
    ) -> anyhow::Result<DatabaseConnection> {
        let db_url = format!(
            "sqlite://{}/{}.db?mode=rwc",
            self.db_path.trim_end_matches('/'),
            name
        );
        let conn = Database::connect(&db_url).await?;
        if conn.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
            let _ = conn
                .execute_unprepared(
                    "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;",
                )
                .await;
        }
        let now_ms = now_ms_u64() as i64;
        let main_db = get_main_db()?;
        let existing = dbreg_entity::Entity::find()
            .filter(dbreg_entity::Column::Name.eq(name))
            .one(main_db)
            .await?;
        if existing.is_none() {
            let active = dbreg_entity::ActiveModel {
                name: Set(name.to_owned()),
                db_connections: Set(Some(1)),
                max_lifetime_ms: Set(max_lifetime_ms),
                created_at: Set(now_ms),
                ..Default::default()
            };
            let result = active.insert(main_db).await?;
            info!(target: "db", name = %result.name, id = result.id, "Database registered");
        } else {
            let existing_model = existing.unwrap();
            let current_conns = existing_model.db_connections.unwrap_or(0).saturating_add(1);
            let mut active: dbreg_entity::ActiveModel = existing_model.into();
            active.db_connections = Set(Some(current_conns));
            if max_lifetime_ms.is_some() {
                active.max_lifetime_ms = Set(max_lifetime_ms);
            }
            active.update(main_db).await?;
        }
        {
            let mut pools = self.pools.write().await;
            pools.insert(
                name.to_owned(),
                Arc::new(TrackedConnection {
                    conn: conn.clone(),
                    last_used_ms: AtomicU64::new(now_ms_u64()),
                }),
            );
        }
        info!(target: "db", name = %name, "Database connection created");
        Ok(conn)
    }

    /// 移除数据库连接并清理磁盘文件
    ///
    /// - `name` — 数据库名称
    /// - 返回值：成功返回 `Ok(())`
    ///
    /// 内部步骤：
    /// 1. 从内存池中移除连接
    /// 2. 从 `db_registry` 表删除对应行
    /// 3. 删除 .db 文件及关联的 -wal、-shm 文件
    ///
    /// # Errors
    ///
    /// 当 `db_registry` 表查询或删除失败时返回错误
    pub async fn remove_conn(&self, name: &str) -> anyhow::Result<()> {
        {
            let mut pools = self.pools.write().await;
            pools.remove(name);
        }
        let main_db = get_main_db()?;
        if let Some(model) = dbreg_entity::Entity::find()
            .filter(dbreg_entity::Column::Name.eq(name))
            .one(main_db)
            .await?
            && let Err(e) = dbreg_entity::Entity::delete_by_id(model.id)
                .exec(main_db)
                .await
        {
            warn!(target: "db", name = %name, error = %e, "Failed to delete db_registry row");
        }
        let db_file = self.get_db_path(name);
        if std::path::Path::new(&db_file).exists()
            && let Err(e) = std::fs::remove_file(&db_file)
        {
            warn!(target: "db", path = %db_file, error = %e, "Failed to delete db file");
        }
        for ext in &["-wal", "-shm"] {
            let f = format!("{db_file}{ext}");
            if std::path::Path::new(&f).exists()
                && let Err(e) = std::fs::remove_file(&f)
            {
                warn!(target: "db", path = %f, error = %e, "Failed to delete WAL/SHM file");
            }
        }
        info!(target: "db", name = %name, "Database connection removed and files cleaned");
        Ok(())
    }

    /// 列出所有已注册数据库的详细信息
    ///
    /// - 返回值：按名称升序排列的 `DbInfo` 列表，含活跃状态
    ///
    /// # Errors
    ///
    /// 当 `db_registry` 表查询失败时返回错误
    pub async fn list_all(&self) -> anyhow::Result<Vec<DbInfo>> {
        let main_db = get_main_db()?;
        let entries = dbreg_entity::Entity::find()
            .order_by(dbreg_entity::Column::Name, sea_orm::Order::Asc)
            .all(main_db)
            .await?;
        let pools = self.pools.read().await;
        Ok(entries
            .iter()
            .map(|e| DbInfo {
                id: e.id,
                name: e.name.clone(),
                file_path: self.get_db_path(&e.name),
                db_connections: e.db_connections,
                max_lifetime_ms: e.max_lifetime_ms,
                created_at: e.created_at,
                is_active: pools.contains_key(&e.name),
            })
            .collect())
    }

    /// 通知清理循环停止并等待其退出（5 秒超时）
    ///
    /// - 设置 `cancelled` 标志并通过 `cancel_notify` 唤醒清理循环
    /// - 最多等待 5 秒，超时则输出警告
    ///
    /// # Panics
    ///
    /// 若内部 Mutex 被 poison（仅当其他持有者在持锁期间 panic 时可能发生）则会 panic
    pub async fn shutdown(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancel_notify.notify_one();
        let handle = self.cleanup_handle.lock().unwrap().take();
        if let Some(handle) = handle {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => info!(target: "db", "DbRegistry cleanup loop exited cleanly"),
                Ok(Err(e)) => {
                    warn!(target: "db", error = %e, "DbRegistry cleanup loop task panicked");
                }
                Err(_) => {
                    warn!(target: "db", "DbRegistry cleanup loop did not exit within 5s timeout");
                }
            }
        }
    }
}

/// 数据库信息摘要，用于 `db.list` RPC 返回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbInfo {
    /// `db_registry` 表主键 `ID`
    pub id: i64,
    /// 数据库名称（同时也是文件名前缀）
    pub name: String,
    /// `SQLite` 数据库文件绝对路径
    pub file_path: String,
    /// 当前连接数引用计数，`None` 表示未跟踪
    pub db_connections: Option<i32>,
    /// 连接最大生命周期（毫秒），`None` 表示永不过期
    pub max_lifetime_ms: Option<i64>,
    /// 创建时间，UNIX 时间戳（毫秒）
    pub created_at: i64,
    /// 连接是否在内存池中活跃
    pub is_active: bool,
}

/// SQL 执行结果，用于 RPC 响应序列化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbExecResult {
    /// 执行是否成功
    pub success: bool,
    /// 查询返回的行数据，每行为 JSON 对象
    pub data: Vec<serde_json::Value>,
    /// 匹配/影响的总行数
    pub row_count: u64,
}

/// 获取主库连接，未初始化时返回错误
fn get_main_db() -> anyhow::Result<&'static sea_orm::DatabaseConnection> {
    get_db().context("Main DB not initialized")
}

/// 将 `SeaORM` 原始查询行转换为 JSON 对象
///
/// - `r` — `SeaORM` `QueryResult` 行
/// - 返回值：`{"col1": val1, "col2": val2, ...}` 形式的 JSON Object
///
/// 内部步骤：
/// 1. 获取所有列名
/// 2. 逐列尝试按类型解析（String → i64 → u32 → f64 → bool → Vec<u8> → JSON）
/// 3. 无法解析的列置为 Null
pub fn row_to_json(r: &sea_orm::QueryResult) -> serde_json::Value {
    let cols = r.column_names();
    let mut map = serde_json::Map::with_capacity(cols.len());
    for col in cols {
        let val = try_column_as_json(r, &col);
        map.insert(col.clone(), val);
    }
    serde_json::Value::Object(map)
}

/// 逐类型尝试将单列值转换为 JSON，按常见类型优先级依次尝试
///
/// 尝试顺序：`String` → `i64` → `u32` → `f64` → `bool` → `Vec<u8>`（hex 或嵌套 JSON）→ `serde_json::Value` → Null
fn try_column_as_json(r: &sea_orm::QueryResult, col: &str) -> serde_json::Value {
    if let Ok(v) = r.try_get::<Option<String>>("", col) {
        return v.map_or(serde_json::Value::Null, serde_json::Value::String);
    }
    if let Ok(v) = r.try_get::<Option<i64>>("", col) {
        return v.map_or(serde_json::Value::Null, |n| {
            serde_json::Value::Number(n.into())
        });
    }
    if let Ok(v) = r.try_get::<Option<u32>>("", col) {
        return v.map_or(serde_json::Value::Null, |n| {
            serde_json::Value::Number(n.into())
        });
    }
    if let Ok(v) = r.try_get::<Option<f64>>("", col) {
        return v.map_or(serde_json::Value::Null, |n| {
            serde_json::Number::from_f64(n)
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        });
    }
    if let Ok(v) = r.try_get::<Option<bool>>("", col) {
        return v.map_or(serde_json::Value::Null, serde_json::Value::Bool);
    }
    if let Ok(v) = r.try_get::<Option<Vec<u8>>>("", col) {
        return match v {
            Some(bytes) => {
                if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    j
                } else {
                    serde_json::Value::String(hex::encode(&bytes))
                }
            }
            None => serde_json::Value::Null,
        };
    }
    if let Ok(v) = r.try_get::<Option<serde_json::Value>>("", col) {
        return v.unwrap_or(serde_json::Value::Null);
    }
    serde_json::Value::Null
}

/// 将 JSON 值转换为 `SeaORM` `Value`，用于 SQL 参数绑定
///
/// - `json` — 输入的 JSON 值
/// - 返回值：对应的 `SeaORM` `Value` 枚举变体
///
/// 类型映射：Null→Json(None)、Bool→Bool、Number→BigInt/BigUnsigned/Double、String→String、Array/Object→Json
#[must_use]
pub fn json_to_sea_value(json: &serde_json::Value) -> sea_orm::Value {
    match json {
        serde_json::Value::Null => sea_orm::Value::Json(None),
        serde_json::Value::Bool(b) => sea_orm::Value::Bool(Some(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                sea_orm::Value::BigInt(Some(i))
            } else if let Some(u) = n.as_u64() {
                sea_orm::Value::BigUnsigned(Some(u))
            } else if let Some(f) = n.as_f64() {
                sea_orm::Value::Double(Some(f))
            } else {
                sea_orm::Value::String(Some(n.to_string()))
            }
        }
        serde_json::Value::String(s) => sea_orm::Value::String(Some(s.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            sea_orm::Value::Json(Some(Box::new(json.clone())))
        }
    }
}

/// 判断 SQL 语句是否为只读查询
///
/// - `sql` — 待判定的 SQL 语句
/// - 返回值：若为 SELECT / PRAGMA / EXPLAIN / WITH 开头则返回 true
///
/// 注意：仅检查语句开头，CTE 后接 DML 的情况仍返回 true（保守策略）
#[must_use]
pub fn is_read_query(sql: &str) -> bool {
    let s = sql.trim_start_matches(|c: char| c.is_whitespace() || c == '(' || c == ';');
    starts_with_ascii_ci(s, "SELECT")
        || starts_with_ascii_ci(s, "PRAGMA")
        || starts_with_ascii_ci(s, "EXPLAIN")
        || starts_with_ascii_ci(s, "WITH")
}

/// ASCII 大小写不敏感的前缀匹配
fn starts_with_ascii_ci(s: &str, prefix: &str) -> bool {
    s.as_bytes()
        .iter()
        .zip(prefix.as_bytes())
        .all(|(a, b)| a.to_ascii_uppercase() == *b)
}
