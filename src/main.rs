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
mod utils;

#[tokio::main]
async fn main() {
    let db_url = "sqlite://test.db?mode=rwc";
    let db = Database::connect(db_url).await.unwrap();

    Migrator::up(&db, None).await.unwrap();
    println!("Migration completed!");

    let uuid = Uuid::new_v4();

    loop {
        let data = StaticMonitoringData::get().await;
        let data_for_db = StaticMonitoringDataForDatabase {
            id: 0,
            uuid,
            data,
            time: get_local_timestamp_ms(),
        };
        insert_static_monitoring_data(
            &db,
            data_for_db,
            &HashSet::from([StaticDataSelector::Cpu, StaticDataSelector::System]),
        )
        .await
        .unwrap();
        println!("Inserted static monitoring data.");

        let filter = MonitoringQueryFilter::new();

        let read = read_static_monitoring_data(
            &db,
            filter.uuid(uuid),
            &HashSet::from([StaticDataSelector::System]),
        )
        .await
        .unwrap();
        println!("Read static monitoring data: {:?}", read);

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
