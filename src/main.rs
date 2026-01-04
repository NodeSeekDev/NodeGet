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
mod launch;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    app_start()?;
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
//app_start for app start action  init config parser args etc...
fn app_start() -> Result<(), Box<dyn std::error::Error>> {
    launch::app_launch()
}
