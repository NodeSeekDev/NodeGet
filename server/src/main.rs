#![feature(duration_millis_float)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

use log::info;
use std::str::FromStr;
use nodeget_lib::args_parse::server::{ServerArgs, ServerCommand};
use crate::rpc_timing::parse_rpc_timing_log_level;
#[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

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
mod kv;
mod rpc_timing;
mod subcommands;
mod token;

// 全局数据库连接单例
pub static DB: tokio::sync::OnceCell<sea_orm::DatabaseConnection> =
    tokio::sync::OnceCell::const_new();

// 全局服务器配置单例
static SERVER_CONFIG: std::sync::OnceLock<nodeget_lib::config::server::ServerConfig> =
    std::sync::OnceLock::new();

// 服务器主函数
//
// 该函数启动 NodeGet 服务器，初始化配置、日志、数据库连接、超级令牌，
// 然后设置 RPC 服务和 WebSocket 终端处理器，并最终启动 HTTP 服务器。
#[tokio::main]
async fn main() {
    println!("Starting nodeget-server");

    let args = ServerArgs::par();

    // Config Parse
    let config = nodeget_lib::config::server::ServerConfig::get_and_parse_config(args.config_path())
        .await
        .unwrap();

    // Log init
    let base_log_level = log::LevelFilter::from_str(&config.log_level)
        .unwrap_or_else(|_| panic!("Invalid log_level '{}'", config.log_level));
    let (rpc_timing_log_level, invalid_rpc_timing_log_level) =
        parse_rpc_timing_log_level(config.jsonrpc_timing_log_level.as_deref());

    simple_logger::SimpleLogger::new()
        .with_level(base_log_level)
        .with_module_level(
            "nodeget_server::rpc_timing",
            rpc_timing_log_level.to_level_filter(),
        )
        .init()
        .unwrap();

    if let Some(invalid_level) = invalid_rpc_timing_log_level {
        log::warn!("Invalid jsonrpc_timing_log_level '{invalid_level}', fallback to 'trace'");
    }

    // Jemalloc Mem Debug
    #[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
    if matches!(&args.command, ServerCommand::Serve { .. }) {
        tokio::spawn(async {
            loop {
                use tikv_jemalloc_ctl::{epoch, stats};
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if epoch::advance().is_err() {
                    return;
                }

                let allocated = stats::allocated::read().unwrap();
                let active = stats::active::read().unwrap();
                let resident = stats::resident::read().unwrap();
                let mapped = stats::mapped::read().unwrap();

                info!(
                    "MEM STATS (Jemalloc Only): App Logic: {:.2} MB | Allocator Active: {:.2} MB | RSS (Resident): {:.2} MB | Mapped: {:.2} MB",
                    allocated as f64 / 1024.0 / 1024.0,
                    active as f64 / 1024.0 / 1024.0,
                    resident as f64 / 1024.0 / 1024.0,
                    mapped as f64 / 1024.0 / 1024.0
                );
            }
        });
    }

    info!("Starting nodeget-server with config: {config:?}");

    // 初始化全局 Config
    SERVER_CONFIG.set(config.clone()).unwrap();

    // 连接数据库
    db_connection::init_db_connection().await;

    match args.command {
        ServerCommand::Serve { .. } => {
            subcommands::serve::run(&config, rpc_timing_log_level).await;
        }
        ServerCommand::Init { .. } => {
            subcommands::init::run().await;
        }
        ServerCommand::RollSuperToken { .. } => {
            subcommands::roll_super_token::run().await;
        }
    }
}
