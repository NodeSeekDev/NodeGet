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

use crate::db_connection::init_db_connection;
use crate::rpc::agent::RpcServer as AgentRpcServer;
use crate::rpc::nodeget::RpcServer as NodegetRpcServer;
use crate::rpc::task::{RpcServer, TaskManager};
use jsonrpsee::server::ServerBuilder;
use log::{info, Level};
use nodeget_lib::config::server::ServerConfig;
use nodeget_lib::utils::compare_uuid;
use sea_orm::DatabaseConnection;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

mod db_connection;
mod entity;
mod rpc;

static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();
static SERVER_CONFIG: OnceLock<ServerConfig> = OnceLock::new();

#[tokio::main]
async fn main() {
    println!("Starting nodeget-server");

    let config = ServerConfig::get_and_parse_config("./config.toml")
        .await
        .unwrap();

    simple_logger::init_with_level(Level::from_str(&config.log_level).unwrap()).unwrap();

    let _ = compare_uuid(config.server_uuid);

    info!("Starting nodeget-server with config: {config:?}");

    SERVER_CONFIG.set(config.clone()).unwrap();

    init_db_connection().await;

    let server = ServerBuilder::default()
        .build(config.ws_listener.parse::<SocketAddr>().unwrap())
        .await
        .unwrap();

    let task_manager = TaskManager::new();

    let mut module = rpc::nodeget::NodegetServerRpcImpl.into_rpc();
    module.merge(rpc::agent::AgentRpcImpl.into_rpc()).unwrap();
    module
        .merge(
            rpc::task::TaskRpcImpl {
                manager: task_manager.clone(),
            }
                .into_rpc(),
        )
        .unwrap();

    let handle = server.start(module);
    handle.stopped().await;
}
