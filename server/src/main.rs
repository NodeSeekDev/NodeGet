#![feature(duration_millis_float)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code
)]

use crate::rpc::agent::RpcServer as AgentRpcServer;
use crate::rpc::nodeget::RpcServer as NodeGetRpcServer;
use crate::rpc::task::RpcServer as TaskRpcServer;
use crate::rpc::token::RpcServer as TokenRpcServer;
use axum::routing::any;
use log::info;
use std::str::FromStr;
use tower::Service;

use crate::token::super_token::generate_super_token;
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
mod token;

// 全局数据库连接单例
static DB: tokio::sync::OnceCell<sea_orm::DatabaseConnection> = tokio::sync::OnceCell::const_new();
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

    // Config Parse
    let config = nodeget_lib::config::server::ServerConfig::get_and_parse_config("./config.toml")
        .await
        .unwrap();

    // Log init
    simple_logger::init_with_level(log::Level::from_str(&config.log_level).unwrap()).unwrap();

    // Jemalloc Mem Debug
    #[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
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

    // 对比 Uuid，发送警告
    let _ =
        nodeget_lib::utils::uuid::compare_uuid(config.server_uuid);

    info!("Starting nodeget-server with config: {config:?}");

    // 初始化全局 Config
    SERVER_CONFIG.set(config.clone()).unwrap();

    // 连接数据库
    db_connection::init_db_connection().await;

    // Show Super Token
    {
        let token = match generate_super_token().await {
            Ok(token) => token,
            Err(e) => {
                panic!("Failed to generate super token: {e}");
            }
        };

        match token {
            Some(token) => {
                info!("Super Token: {}", token.0);
                info!("Root Password: {}", token.1);
            }
            None => {
                info!("Super Token has been generated.");
            }
        }
    }

    let task_manager = rpc::task::TaskManager::new();
    let terminal_state = terminal::TerminalState {
        sessions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    };

    let mut rpc_module = rpc::nodeget::NodegetServerRpcImpl.into_rpc();
    rpc_module
        .merge(rpc::agent::AgentRpcImpl.into_rpc())
        .unwrap();
    rpc_module
        .merge(
            rpc::task::TaskRpcImpl {
                manager: task_manager.clone(),
            }
            .into_rpc(),
        )
        .unwrap();
    rpc_module
        .merge(rpc::token::TokenRpcImpl.into_rpc())
        .unwrap();

    let (stop_handle, _server_handle) = jsonrpsee::server::stop_channel();

    let jsonrpc_service = jsonrpsee::server::Server::builder()
        .set_config(
            jsonrpsee::server::ServerConfig::builder()
                .max_response_body_size(u32::MAX)
                .max_request_body_size(u32::MAX)
                .build(),
        )
        .to_service_builder()
        .build(rpc_module, stop_handle);

    let app = axum::Router::new()
        .route("/terminal", any(terminal::terminal_ws_handler))
        .with_state(terminal_state)
        .fallback(any(move |req: axum::extract::Request| {
            let mut rpc_service = jsonrpc_service.clone();
            async move { rpc_service.call(req).await.unwrap() }
        }));

    let listener =
        tokio::net::TcpListener::bind(config.ws_listener.parse::<std::net::SocketAddr>().unwrap())
            .await
            .unwrap();

    axum::serve(listener, app).await.unwrap();
}
