pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260113_044428_create_static_monitoring::Migration),
            Box::new(m20260115_131325_create_dynamic_monitoring::Migration),
        ]
    }
}
mod m20260113_044428_create_static_monitoring;
mod m20260115_131325_create_dynamic_monitoring;
