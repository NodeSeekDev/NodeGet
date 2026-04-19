pub mod clean_up;

use crate::{DB, SERVER_CONFIG};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use std::process;
use std::time::Duration;
use tracing::log::LevelFilter;
use tracing::{debug, error, info};

// 初始化数据库连接并应用迁移
//
// 该函数连接到数据库，应用必要的迁移，并根据数据库类型进行特定配置。
// 如果配置无效或连接失败，则会记录错误并退出进程。
pub async fn init_db_connection() {
    info!(target: "db", "initializing database connection");
    let config_guard = SERVER_CONFIG
        .get()
        .expect("Server config not initialized")
        .read()
        .expect("SERVER_CONFIG lock poisoned");

    let db_url = config_guard.database.database_url.clone();
    let connect_timeout = config_guard.database.connect_timeout_ms.unwrap_or(3000);
    let acquire_timeout = config_guard.database.acquire_timeout_ms.unwrap_or(3000);
    let idle_timeout = config_guard.database.idle_timeout_ms.unwrap_or(3000);
    let max_lifetime = config_guard.database.max_lifetime_ms.unwrap_or(30000);
    let max_connections = config_guard.database.max_connections.unwrap_or(10);
    drop(config_guard);

    DB.get_or_init(|| async {
        let mut opt = ConnectOptions::new(&db_url);
        opt.sqlx_logging_level(LevelFilter::Trace)
            .connect_timeout(Duration::from_millis(connect_timeout))
            .acquire_timeout(Duration::from_millis(acquire_timeout))
            .idle_timeout(Duration::from_millis(idle_timeout))
            .max_lifetime(Duration::from_millis(max_lifetime))
            .max_connections(max_connections);

        debug!(
            target: "db",
            connect_timeout,
            acquire_timeout,
            idle_timeout,
            max_lifetime,
            max_connections,
            "Database connection options configured"
        );

        let db = Database::connect(opt).await.unwrap_or_else(|e| {
            error!(target: "db", error = %e, "Unable to connect to the database");
            process::exit(1);
        });

        info!(target: "db", "Database connected successfully");

        Migrator::up(&db, None).await.unwrap_or_else(|e| {
            error!(target: "db", error = %e, "Unable to apply migrations");
            process::exit(1);
        });

        info!(target: "db", "Migrations applied successfully");

        if db.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
            let _ = db
                .execute_unprepared("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
                .await;
            info!(target: "db", "SQLite WAL mode enabled");
        }

        db
    })
    .await;
}
