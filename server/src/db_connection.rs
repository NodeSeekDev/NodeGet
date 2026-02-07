use crate::{DB, SERVER_CONFIG};
use log::{LevelFilter, error, info};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use std::process;
use std::str::FromStr;
use std::time::Duration;

// 初始化数据库连接并应用迁移
//
// 该函数连接到数据库，应用必要的迁移，并根据数据库类型进行特定配置。
// 如果配置无效或连接失败，则会记录错误并退出进程。
pub async fn init_db_connection() {
    let config = SERVER_CONFIG.get().expect("Server config not initialized");

    DB.get_or_init(|| async {
        let log_level_str = config.database.sqlx_log_level.clone().unwrap_or_else(|| "info".to_string());
        let log_level = LevelFilter::from_str(&log_level_str).unwrap_or_else(|_| {
            error!("Configuration error: Invalid sqlx_log_level '{log_level_str}'");
            process::exit(1);
        });

        let mut opt = ConnectOptions::new(&config.database.database_url);
        opt.sqlx_logging_level(log_level)
            .connect_timeout(Duration::from_millis(config.database.connect_timeout_ms.unwrap_or(3000)))
            .acquire_timeout(Duration::from_millis(config.database.acquire_timeout_ms.unwrap_or(3000)))
            .idle_timeout(Duration::from_millis(config.database.idle_timeout_ms.unwrap_or(3000)))
            .max_lifetime(Duration::from_millis(config.database.max_lifetime_ms.unwrap_or(30000)))
            .max_connections(config.database.max_connections.unwrap_or(10));

        let db = Database::connect(opt).await.unwrap_or_else(|e| {
            error!("Unable to connect to the database: {e}");
            process::exit(1);
        });

        info!("Database connected successfully.");

        Migrator::up(&db, None).await.unwrap_or_else(|e| {
            error!("Unable to apply migrations: {e}");
            process::exit(1);
        });

        info!("Migrations applied successfully.");

        if db.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
            let _ = db
                .execute_unprepared("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
                .await;
            info!("SQLite WAL mode enabled.");
        }

        db
    })
    .await;
}
