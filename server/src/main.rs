#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

use ng_config::args_parse::server::{ServerArgs, ServerCommand};
use ng_core::utils::version::NodeGetVersion;
use tracing::info;

// 服务器专属模块（不属于任何 ng-* crate）
mod logging;
mod rpc_nodeget;
mod rpc_timing;
mod subcommands;

// 服务器主函数
//
// 该函数启动 NodeGet 服务器，初始化配置、日志、数据库连接、超级令牌，
// 然后设置 RPC 服务和 WebSocket 终端处理器，并最终启动 HTTP 服务器。
fn main() {
    println!("Starting nodeget-server");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .global_queue_interval(3)
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");

    runtime.block_on(async_main());
}

async fn async_main() {
    ng_js_runtime::init_server_runtime(tokio::runtime::Handle::current());

    let args = ServerArgs::par();

    {
        if args.command == ServerCommand::Version {
            let version = NodeGetVersion::get();
            println!("{version}");
            return;
        }
    }

    let config_path = args.config_path().to_owned();
    let _ = ng_config::set_server_config_path(config_path.clone());
    ng_config::init_reload_notify();

    // Config Parse
    let mut config =
        match ng_config::config::server::ServerConfig::get_and_parse_config(&config_path).await {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Failed to parse config: {e}");
                std::process::exit(1);
            }
        };

    // Log init
    logging::init(config.logging.as_ref());

    info!(target: "server", config = ?config, "Starting nodeget-server");

    // 初始化全局 Config
    if let Err(e) = ng_config::set_server_config(config.clone()) {
        tracing::error!(target: "server", error = %e, "Failed to update global config");
        std::process::exit(1);
    }

    match args.command {
        ServerCommand::Serve { .. } => {
            init_db_connection().await;
            loop {
                subcommands::serve::run(&config).await;

                let reloaded_config =
                    match ng_config::config::server::ServerConfig::get_and_parse_config(
                        &config_path,
                    )
                    .await
                    {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            tracing::error!(
                                target: "server",
                                error = %e,
                                "Failed to reload config after edit, keeping current config"
                            );
                            continue; // 保留当前配置，继续循环
                        }
                    };
                if let Err(e) = ng_config::set_server_config(reloaded_config.clone()) {
                    tracing::error!(
                        target: "server",
                        error = %e,
                        "Failed to update global config after reload, keeping current config"
                    );
                    continue;
                }
                config = reloaded_config;
                info!(target: "server", "Config hot reload applied");
            }
        }
        ServerCommand::Init { .. } => {
            init_db_connection().await;
            subcommands::init::run().await;
        }
        ServerCommand::RollSuperToken { .. } => {
            init_db_connection().await;
            subcommands::roll_super_token::run().await;
        }
        ServerCommand::GetUuid { .. } => {
            subcommands::get_uuid::run(&config);
        }
        ServerCommand::Version => {
            let version = NodeGetVersion::get();
            println!("{version}");
        }
    }
}

/// 初始化数据库连接，从全局配置读取参数
async fn init_db_connection() {
    let db_config = {
        let config_guard = ng_config::get_server_config()
            .expect("Server config not initialized")
            .read()
            .expect("SERVER_CONFIG lock poisoned");

        ng_db::DbConnectionConfig {
            database_url: config_guard.database.database_url.clone(),
            connect_timeout_ms: config_guard.database.connect_timeout_ms.unwrap_or(3000),
            acquire_timeout_ms: config_guard.database.acquire_timeout_ms.unwrap_or(3000),
            idle_timeout_ms: config_guard.database.idle_timeout_ms.unwrap_or(3000),
            max_lifetime_ms: config_guard.database.max_lifetime_ms.unwrap_or(30000),
            max_connections: config_guard.database.max_connections.unwrap_or(10),
        }
    };

    ng_db::init_db_connection(db_config)
        .await
        .expect("Failed to initialize database connection");
}
