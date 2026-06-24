//! 数据库连接初始化
//!
//! 负责根据配置建立主库连接、执行 `SeaORM` 迁移，并对 `SQLite` 启用 WAL 等优化 PRAGMA。
//! `SQLite` PRAGMA 通过连接建立后执行语句设置；连接池轮换时新连接需重新执行。
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
            idle_timeout_ms: 60000,
            max_lifetime_ms: 1_800_000,
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

    // SQLite: auto_vacuum 必须在库为空（建库阶段）设置才生效，老库会被忽略。
    // 故在 Migrator::up 建表之前抢先设置，确保新库建库即启用 INCREMENTAL。
    // 对已有数据的老库，此设置不生效（auto_vacuum 值保持 NONE），需用官方迁移教程转库。
    if db.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        let _ = db.execute_unprepared("PRAGMA auto_vacuum = INCREMENTAL;").await;
    }

    Migrator::up(&db, None).await.map_err(|e| {
        error!(target: "db", error = %e, "Unable to apply migrations");
        e
    })?;

    info!(target: "db", "Migrations applied successfully");

    // SQLite: 通过 PRAGMA 语句设置性能优化参数
    // 注意：PRAGMA 仅对当前连接有效，连接池轮换新连接时不会自动继承。
    // 但 WAL 模式和 cache_size 是持久化/数据库级设置，设置一次即可全局生效；
    // busy_timeout 和 foreign_keys 是连接级设置，连接池新连接需要重新设置。
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
        db.execute_unprepared("PRAGMA cache_size = -64000;").await?;
        // 读回 auto_vacuum 当前值（0=NONE,1=FULL,2=INCREMENTAL）用于日志诊断。
        // 老库即便上面设了 INCREMENTAL 也不会改变值，运维可凭此判断是否需走迁移教程。
        let auto_vacuum = db
            .query_one_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA auto_vacuum",
                [],
            ))
            .await
            .ok()
            .flatten()
            .and_then(|row| row.try_get::<Option<i64>>("", "auto_vacuum").ok().flatten())
            .unwrap_or(-1);
        info!(target: "db", "SQLite PRAGMAs applied: WAL, synchronous=NORMAL, busy_timeout=5000, foreign_keys=ON, cache_size=-64000; auto_vacuum={}", auto_vacuum);
    }

    set_db(db);
    Ok(())
}

/// 为 `SQLite` URL 追加 `mode` 查询参数（`SQLx` 仅支持 `mode` 参数）。
///
/// 若 URL 中已包含 `mode=`，则不重复追加。
/// 其他 `PRAGMA`（`journal_mode`、`synchronous` 等）不可作为 URL 参数，
/// `SQLx` 驱动不支持，会报 `unknown query parameter` 错误。
fn build_sqlite_url_with_mode(url: &str) -> String {
    // 若已有 mode 参数则不追加
    if url.contains("mode=") {
        return url.to_owned();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}mode=rwc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_url_no_existing_params() {
        let result = build_sqlite_url_with_mode("sqlite://./data.db");
        assert_eq!(result, "sqlite://./data.db?mode=rwc");
    }

    #[test]
    fn sqlite_url_with_existing_mode() {
        let result = build_sqlite_url_with_mode("sqlite://./data.db?mode=rwc");
        assert_eq!(result, "sqlite://./data.db?mode=rwc");
    }

    #[test]
    fn sqlite_url_with_existing_other_params() {
        let result = build_sqlite_url_with_mode("sqlite://./data.db?timeout=3000");
        assert_eq!(result, "sqlite://./data.db?timeout=3000&mode=rwc");
    }

    #[test]
    fn sqlite_url_no_double_mode() {
        let result = build_sqlite_url_with_mode("sqlite://nodeget.db?mode=ro");
        assert_eq!(result, "sqlite://nodeget.db?mode=ro");
    }
}
