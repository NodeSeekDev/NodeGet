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
                    .table(StaticMonitoringInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::UuidId)
                            .small_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::CpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::SystemData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::GpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::DataHash)
                            .binary_len(16)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-static-uuid-timestamp")
                    .table(StaticMonitoringInDatabase::Table)
                    .col(StaticMonitoringInDatabase::UuidId)
                    .col(StaticMonitoringInDatabase::Timestamp)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-static-uuid-data-hash")
                    .table(StaticMonitoringInDatabase::Table)
                    .col(StaticMonitoringInDatabase::UuidId)
                    .col(StaticMonitoringInDatabase::DataHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres => {
                let db = manager.get_connection();
                db.execute_unprepared(
                    "ALTER TABLE static_monitoring
                        ALTER COLUMN cpu_data SET COMPRESSION lz4,
                        ALTER COLUMN system_data SET COMPRESSION lz4,
                        ALTER COLUMN gpu_data SET COMPRESSION lz4;",
                )
                .await?;
            }
            DbBackend::Sqlite => {}
            _ => {}
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(StaticMonitoringInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum StaticMonitoringInDatabase {
    #[sea_orm(iden = "static_monitoring")]
    Table,
    Id,
    UuidId,
    Timestamp,

    CpuData,
    SystemData,
    GpuData,
    DataHash,
}
