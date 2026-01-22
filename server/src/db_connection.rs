use crate::{DB, SERVER_CONFIG};
use log::{error, info, LevelFilter};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, Database};
use std::process;
use std::str::FromStr;
use std::time::Duration;

pub async fn init_db_connection() {
    let config = SERVER_CONFIG.get().expect("Server config not initialized");

    DB.get_or_init(|| async {
        let Ok(log_level) = LevelFilter::from_str(
            &config
                .database
                .sqlx_log_level
                .clone()
                .unwrap_or("info".to_string()),
        ) else {
            error!(
                "Configuration error: Invalid sqlx_log_level '{}'",
                &config
                    .database
                    .sqlx_log_level
                    .clone()
                    .unwrap_or("info".to_string())
            );
            process::exit(1);
        };

        let mut opt = ConnectOptions::new(&config.database.database_url);
        opt.sqlx_logging_level(log_level);

        opt.connect_timeout(Duration::from_millis(
            config.database.connect_timeout_ms.unwrap_or(10000),
        ));
        opt.acquire_timeout(Duration::from_millis(
            config.database.acquire_timeout_ms.unwrap_or(10000),
        ));
        opt.idle_timeout(Duration::from_millis(
            config.database.idle_timeout_ms.unwrap_or(10000),
        ));

        opt.max_connections(config.database.max_connections.unwrap_or(100));

        let db = match Database::connect(opt).await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Unable to connect to the database: {e}");
                process::exit(1);
            }
        };

        info!("Database connected successfully.");

        if let Err(e) = Migrator::up(&db, None).await {
            error!("Unable to apply migrations: {e}");
            process::exit(1);
        }

        info!("Migrations applied successfully.");

        db
    })
        .await;
}
