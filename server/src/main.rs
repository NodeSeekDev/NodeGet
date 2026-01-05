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

use crate::monitoring::data_structure::{StaticMonitoringData, StaticMonitoringDataForDatabase};
use crate::monitoring::database::{
    MonitoringQueryFilter, StaticDataSelector, insert_static_monitoring_data,
    read_static_monitoring_data,
};
use crate::utils::get_local_timestamp_ms;
use migration::{Migrator, MigratorTrait};
use sea_orm::*;
use std::collections::HashSet;
use uuid::Uuid;

mod entities;
mod monitoring;
mod launch;
mod utils;

#[tokio::main]
async fn main() {
    app_start().unwrap();
    let db_url = "sqlite://test.db?mode=rwc";
    let db = Database::connect(db_url).await.unwrap();

    Migrator::up(&db, None).await.unwrap();
    println!("Migration completed!");
}

fn app_start() -> Result<(), Box<dyn std::error::Error>> {
    launch::app_launch()
}
