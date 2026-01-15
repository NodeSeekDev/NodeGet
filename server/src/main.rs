#![feature(duration_millis_float)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::await_holding_lock,
    dead_code
)]

use crate::rpc::agent::RpcServer as AgentRpcServer;
use crate::rpc::nodeget::RpcServer as NodegetRpcServer;
use jsonrpsee::server::ServerBuilder;
use migration::{Migrator, MigratorTrait};
use sea_orm::{Database, DatabaseConnection};
use std::net::SocketAddr;
use tokio::sync::OnceCell;

mod entity;
mod rpc;

static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    let _db = DB
        .get_or_init(|| async {
            let db_url = "sqlite://test.db?mode=rwc";
            let db = Database::connect(db_url).await.unwrap();
            println!("Database connected.");
            Migrator::up(&db, None).await.unwrap();
            println!("Migrations applied successfully.");
            db
        })
        .await;

    let server = ServerBuilder::default()
        .build("127.0.0.1:3000".parse::<SocketAddr>().unwrap())
        .await
        .unwrap();

    let mut module = rpc::nodeget::NodegetServerRpcImpl.into_rpc();
    module.merge(rpc::agent::AgentRpcImpl.into_rpc()).unwrap();

    let handle = server.start(module);
    handle.stopped().await;
}
