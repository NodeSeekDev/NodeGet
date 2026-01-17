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

use crate::rpc::monitoring_data_report::{
    handle_dynamic_monitoring_data_report, handle_static_monitoring_data_report,
};
use log::{Level, info};
use nodeget_lib::config::agent::AgentConfig;
use std::str::FromStr;
use std::sync::OnceLock;

mod monitoring;
mod rpc;
mod tasks;

static AGENT_CONFIG: OnceLock<AgentConfig> = OnceLock::new();

#[tokio::main]
async fn main() {
    println!("Starting nodeget-agent");

    let config = AgentConfig::get_and_parse_config("./config.toml")
        .await
        .unwrap();

    simple_logger::init_with_level(Level::from_str(&config.log_level).unwrap()).unwrap();

    info!("Starting nodeget-agent with config: {config:?}");

    AGENT_CONFIG.set(config).unwrap();

    //////////

    rpc::multi_server::init_connections(AGENT_CONFIG.get().unwrap().server.clone().unwrap());

    tokio::spawn(async {
        handle_static_monitoring_data_report().await;
    });

    tokio::spawn(async {
        handle_dynamic_monitoring_data_report().await;
    });

    tokio::signal::ctrl_c().await.unwrap();
}
