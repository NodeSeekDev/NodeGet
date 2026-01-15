use sea_orm_migration::{prelude::*, schema::*};

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
                            .string()
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
            .await
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
