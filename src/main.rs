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

use crate::monitoring::data_structure::MonitoringData;
use tokio::time::Instant;

mod monitoring;

#[tokio::main]
async fn main() {
    loop {
        let start = Instant::now();
        let all = MonitoringData::refresh_and_get().await;
        let time = start.elapsed();
        println!("{all:#?}");
        println!("Time: {} millis", time.as_millis_f64());
        println!("Size: {} Bytes", size_of_val(&all));
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
