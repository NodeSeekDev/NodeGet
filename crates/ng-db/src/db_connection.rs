//! 数据库连接初始化
//!
//! 负责根据配置建立主库连接、执行 `SeaORM` 迁移，并对 `SQLite` 启用 WAL 等优化 PRAGMA。
//! 服务端启动流程中由 `serve.rs` 调用 `init_db_connection`。

use crate::set_db;
use ng_db_migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use std::time::Duration;
use tracing::log::LevelFilter;
use tracing::{debug, error, info};

/// 数据库连接配置参数
///
/// 所有超时字段单位为毫秒，由配置文件解析后传入。
pub struct DbConnectionConfig {
    /// 数据库连接 URL，如 `sqlite://./data.db` 或 `postgres://user:pass@host/db`
    pub database_url: String,
    /// 建立连接的超时时间（毫秒）
    pub connect_timeout_ms: u64,
    /// 从连接池获取连接的超时时间（毫秒）
    pub acquire_timeout_ms: u64,
    /// 空闲连接超时时间（毫秒）
    pub idle_timeout_ms: u64,
    /// 连接最大生命周期（毫秒）
    pub max_lifetime_ms: u64,
    /// 连接池最大连接数
    pub max_connections: u32,
}

impl Default for DbConnectionConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            connect_timeout_ms: 3000,
            acquire_timeout_ms: 3000,
            idle_timeout_ms: 3000,
            max_lifetime_ms: 30000,
            max_connections: 10,
        }
    }
}

/// 初始化数据库连接并应用迁移
///
/// - `config` — 连接配置参数
/// - 返回值：成功返回 `Ok(())`，连接或迁移失败返回 `Err`
///
/// 内部步骤：
/// 1. 构建 `ConnectOptions` 并配置超时与池参数
/// 2. 连接数据库
/// 3. 执行 `SeaORM` 迁移（`Migrator::up`）
/// 4. 若为 `SQLite`，依次设置 `WAL`、`synchronous=NORMAL`、`busy_timeout=5000`、`foreign_keys=ON`
/// 5. 将连接写入全局单例（`set_db`）
///
/// # Errors
///
/// 当数据库连接失败、迁移执行失败或 `SQLite` PRAGMA 设置失败时返回错误
pub async fn init_db_connection(config: DbConnectionConfig) -> anyhow::Result<()> {
    info!(target: "db", "initializing database connection");

    let mut opt = ConnectOptions::new(&config.database_url);
    opt.sqlx_logging_level(LevelFilter::Trace)
        .connect_timeout(Duration::from_millis(config.connect_timeout_ms))
        .acquire_timeout(Duration::from_millis(config.acquire_timeout_ms))
        .idle_timeout(Duration::from_millis(config.idle_timeout_ms))
        .max_lifetime(Duration::from_millis(config.max_lifetime_ms))
        .max_connections(config.max_connections);

    debug!(
        target: "db",
        connect_timeout = config.connect_timeout_ms,
        acquire_timeout = config.acquire_timeout_ms,
        idle_timeout = config.idle_timeout_ms,
        max_lifetime = config.max_lifetime_ms,
        max_connections = config.max_connections,
        "Database connection options configured"
    );

    let db = Database::connect(opt).await.map_err(|e| {
        error!(target: "db", error = %e, "Unable to connect to the database");
        e
    })?;

    info!(target: "db", "Database connected successfully");

    Migrator::up(&db, None).await.map_err(|e| {
        error!(target: "db", error = %e, "Unable to apply migrations");
        e
    })?;

    info!(target: "db", "Migrations applied successfully");

    if db.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        db.execute_unprepared("PRAGMA journal_mode=WAL;")
            .await
            .map_err(|e| {
                error!(target: "db", error = %e, "Failed to enable WAL mode");
                e
            })?;
        db.execute_unprepared("PRAGMA synchronous=NORMAL;").await?;
        db.execute_unprepared("PRAGMA busy_timeout = 5000;").await?;
        db.execute_unprepared("PRAGMA foreign_keys = ON;").await?;
        info!(target: "db", "SQLite PRAGMAs applied: WAL, synchronous=NORMAL, busy_timeout=5000, foreign_keys=ON");
    }

    set_db(db);
    Ok(())
}
