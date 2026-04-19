use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dynamic_monitoring_summary")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub uuid_id: i16,
    pub timestamp: i64,
    pub cpu_usage: Option<i16>,
    pub gpu_usage: Option<i16>,
    pub used_swap: Option<i64>,
    pub total_swap: Option<i64>,
    pub used_memory: Option<i64>,
    pub total_memory: Option<i64>,
    pub available_memory: Option<i64>,
    pub load_one: Option<i16>,
    pub load_five: Option<i16>,
    pub load_fifteen: Option<i16>,
    pub uptime: Option<i32>,
    pub boot_time: Option<i64>,
    pub process_count: Option<i32>,
    pub total_space: Option<i64>,
    pub available_space: Option<i64>,
    pub read_speed: Option<i64>,
    pub write_speed: Option<i64>,
    pub tcp_connections: Option<i32>,
    pub udp_connections: Option<i32>,
    pub total_received: Option<i64>,
    pub total_transmitted: Option<i64>,
    pub transmit_speed: Option<i64>,
    pub receive_speed: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
