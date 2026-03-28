pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260113_044428_create_static_monitoring::Migration),
            Box::new(m20260115_131325_create_dynamic_monitoring::Migration),
            Box::new(m20260118_030100_create_task::Migration),
            Box::new(m20260131_112949_create_token::Migration),
            Box::new(m20260205_024306_create_kv::Migration),
            Box::new(m20260206_035808_create_crontab::Migration),
            Box::new(m20260206_040842_create_crontab_result::Migration),
            Box::new(m20260327_150739_create_js_worker::Migration),
            Box::new(m20260328_033808_create_js_result::Migration),
        ]
    }
}
mod m20260113_044428_create_static_monitoring;
mod m20260115_131325_create_dynamic_monitoring;
mod m20260118_030100_create_task;
mod m20260131_112949_create_token;
mod m20260205_024306_create_kv;
mod m20260206_035808_create_crontab;
mod m20260206_040842_create_crontab_result;
mod m20260327_150739_create_js_worker;
mod m20260328_033808_create_js_result;
