use crate::sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DynamicMonitoringInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::Uuid)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::CpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::RamData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::LoadData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::SystemData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::DiskData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::NetworkData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicMonitoringInDatabase::GpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres => {
                let db = manager.get_connection();
                db.execute_unprepared(
                    "ALTER TABLE dynamic_monitoring
                        ALTER COLUMN cpu_data SET COMPRESSION lz4,
                        ALTER COLUMN ram_data SET COMPRESSION lz4,
                        ALTER COLUMN load_data SET COMPRESSION lz4,
                        ALTER COLUMN system_data SET COMPRESSION lz4,
                        ALTER COLUMN disk_data SET COMPRESSION lz4,
                        ALTER COLUMN network_data SET COMPRESSION lz4,
                        ALTER COLUMN gpu_data SET COMPRESSION lz4;",
                )
                    .await?;
            }
            DbBackend::Sqlite => {} // todo!()
            _ => {
                todo!()
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(DynamicMonitoringInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DynamicMonitoringInDatabase {
    #[sea_orm(iden = "dynamic_monitoring")]
    Table,
    Id,
    Uuid,
    Timestamp,

    CpuData,
    RamData,
    LoadData,
    SystemData,
    DiskData,
    NetworkData,
    GpuData,
}
