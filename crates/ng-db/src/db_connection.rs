use crate::set_db;
use ng_db_migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use std::time::Duration;
use tracing::log::LevelFilter;
use tracing::{debug, error, info};

/// Database connection configuration parameters.
pub struct DbConnectionConfig {
    pub database_url: String,
    pub connect_timeout_ms: u64,
    pub acquire_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub max_lifetime_ms: u64,
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

// 初始化数据库连接并应用迁移
//
// 该函数连接到数据库，应用必要的迁移，并根据数据库类型进行特定配置。
// 如果连接失败，则会记录错误并返回 Err。
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
        let _ = db
            .execute_unprepared("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .await;
        info!(target: "db", "SQLite WAL mode enabled");
    }

    set_db(db);
    Ok(())
}
