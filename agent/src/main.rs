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

use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use monitoring::impls::Monitor;

mod monitoring;

#[tokio::main]
async fn main() {
    println!("{}", miniserde::json::to_string(&StaticMonitoringData::refresh_and_get().await));
    println!("{}", miniserde::json::to_string(&DynamicMonitoringData::refresh_and_get().await));
}