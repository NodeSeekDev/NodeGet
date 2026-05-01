#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

use nodeget_lib::args_parse::server::{ServerArgs, ServerCommand};
use nodeget_lib::utils::version::NodeGetVersion;
use tracing::info;

// 数据库连接模块
mod db_connection;
// 实体模块，定义数据库实体
mod entity;
// RPC 接口模块
mod rpc;
// 终端模块，处理终端连接
mod terminal;
// 令牌模块，处理令牌相关功能
mod crontab;
pub mod js_runtime;
mod kv;
mod logging;
pub(crate) mod monitoring_buffer;
pub(crate) mod monitoring_uuid_cache;
mod rpc_timing;
pub(crate) mod static_hash_cache;

pub(crate) mod monitoring_last_cache;
mod subcommands;
mod token;

// 全局数据库连接单例
pub static DB: tokio::sync::OnceCell<sea_orm::DatabaseConnection> =
    tokio::sync::OnceCell::const_new();

// 全局服务器配置单例
static SERVER_CONFIG: std::sync::OnceLock<
    std::sync::RwLock<nodeget_lib::config::server::ServerConfig>,
> = std::sync::OnceLock::new();
pub(crate) static SERVER_CONFIG_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
pub(crate) static RELOAD_NOTIFY: std::sync::OnceLock<tokio::sync::Notify> =
    std::sync::OnceLock::new();

// 服务器主函数
//
// 该函数启动 NodeGet 服务器，初始化配置、日志、数据库连接、超级令牌，
// 然后设置 RPC 服务和 WebSocket 终端处理器，并最终启动 HTTP 服务器。
#[tokio::main]
async fn main() {
    println!("Starting nodeget-server");
    js_runtime::server_runtime::init(tokio::runtime::Handle::current());

    let args = ServerArgs::par();

    {
        if args.command == ServerCommand::Version {
            let version = NodeGetVersion::get();
            println!("{version}");
            return;
        }
    }

    let config_path = args.config_path().to_owned();
    let _ = SERVER_CONFIG_PATH.set(config_path.clone());
    RELOAD_NOTIFY.get_or_init(tokio::sync::Notify::new);

    // Config Parse
    let mut config =
        match nodeget_lib::config::server::ServerConfig::get_and_parse_config(&config_path).await {
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
    if let Err(e) = update_global_config(config.clone()) {
        tracing::error!(target: "server", error = %e, "Failed to update global config");
        std::process::exit(1);
    }

    match args.command {
        ServerCommand::Serve { .. } => {
            db_connection::init_db_connection().await;
            loop {
                subcommands::serve::run(&config).await;

                let reloaded_config =
                    match nodeget_lib::config::server::ServerConfig::get_and_parse_config(
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
                if let Err(e) = update_global_config(reloaded_config.clone()) {
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
            db_connection::init_db_connection().await;
            subcommands::init::run().await;
        }
        ServerCommand::RollSuperToken { .. } => {
            db_connection::init_db_connection().await;
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

fn update_global_config(config: nodeget_lib::config::server::ServerConfig) -> anyhow::Result<()> {
    if let Some(lock) = SERVER_CONFIG.get() {
        {
            let mut guard = lock.write().map_err(|e| anyhow::anyhow!("{e}"))?;
            *guard = config;
        }
        return Ok(());
    }

    SERVER_CONFIG
        .set(std::sync::RwLock::new(config))
        .map_err(|_| anyhow::anyhow!("Failed to set SERVER_CONFIG"))?;
    Ok(())
}
