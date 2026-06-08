//! `NodeGet` 服务器入口
//!
//! 负责解析命令行参数、初始化全局状态（配置、日志、数据库），
//! 并根据子命令分发到对应处理逻辑。
//! Serve 模式下支持配置热重载：收到 `RELOAD_NOTIFY` 信号后重新解析配置文件并重启服务。

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

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// 服务器主函数
///
/// 构建多线程 Tokio 运行时，然后在异步上下文中执行 [`async_main`]。
/// - `global_queue_interval(3)`：多线程调度器全局队列轮询间隔，平衡延迟与吞吐
fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    #[cfg(feature = "dhat-heap")]
    println!("Dhat heap profiler enabled");

    println!("Starting nodeget-server");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .global_queue_interval(3)
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");

    runtime.block_on(async_main());
}

/// 异步主逻辑：解析参数 → 初始化全局配置 → 按子命令分发
///
/// 内部步骤：
/// 1. 初始化 JS 运行时（供 ng-js-runtime 全局使用）
/// 2. 提前处理 Version 子命令（无需加载配置即可返回）
/// 3. 设置配置文件路径、热重载信号通道
/// 4. 解析配置文件并初始化全局 Config
/// 5. 根据 `ServerCommand` 分发：Serve 进入热重载循环，其余为一次性子命令
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

    // 解析配置文件
    let mut config =
        match ng_config::config::server::ServerConfig::get_and_parse_config(&config_path).await {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Failed to parse config: {e}");
                std::process::exit(1);
            }
        };

    // 初始化日志系统
    logging::init(config.logging.as_ref());

    info!(target: "server", config = ?config, "Starting nodeget-server");

    // 写入全局 Config 单例
    if let Err(e) = ng_config::set_server_config(config.clone()) {
        tracing::error!(target: "server", error = %e, "Failed to update global config");
        std::process::exit(1);
    }

    match args.command {
        ServerCommand::Serve { .. } => {
            init_db_connection().await;
            // 热重载循环：serve 退出后重新解析配置，成功则重启服务
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
                ng_static::reload_static_path();
                ng_static::clear_dav_handler_cache();
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

/// 初始化数据库连接
///
/// - 从全局 Config 读取 database 配置段
/// - 未显式配置的超时/连接数参数使用默认值（单位：毫秒/个）
/// - 调用 [`ng_db::init_db_connection`] 写入全局 DB 单例
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
