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

use crate::monitoring::impls::Monitor;
use futures::{SinkExt, StreamExt};
use miniserde::{Deserialize, Serialize};
use nodeget_lib::monitoring::data_structure::DynamicMonitoringData;
use nodeget_lib::utils::get_stable_device_uuid;
use std::env::args;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

mod monitoring;
mod tasks;

static UUID: OnceLock<String> = OnceLock::new();

#[tokio::main]
async fn main() {
    UUID.set(get_stable_device_uuid()).unwrap();

    let (ws_stream, _resp) = timeout(
        Duration::from_secs(3),
        connect_async(
            args().nth(1).unwrap()
        ),
    ).await.unwrap().unwrap();

    let (mut writer, reader) = ws_stream.split();

    #[derive(Serialize, Deserialize)]
    struct JsonRpc {
        jsonrpc: String,
        id: u64,
        method: String,
        params: Vec<miniserde::json::Value>,
    }

    loop {
        let data = DynamicMonitoringData::refresh_and_get().await;

        let jsonrpc = JsonRpc {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "agent_report_dynamic".to_string(),
            params: vec![
                miniserde::json::from_str(r#""1""#).unwrap(),
                miniserde::json::from_str(&miniserde::json::to_string(&data)).unwrap(),
            ]
        };

        let json = miniserde::json::to_string(&jsonrpc);
        println!("{}", json);

        writer.send(Message::Text(Utf8Bytes::from(json))).await.unwrap();



        sleep(Duration::from_secs(1)).await
    }
}
