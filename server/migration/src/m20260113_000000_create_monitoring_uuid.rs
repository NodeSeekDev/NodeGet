use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum MonitoringUuid {
    Table,
    Id,
    Uuid,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MonitoringUuid::Table)
                    .if_not_exists()
                    .col(
                        small_integer(MonitoringUuid::Id)
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(uuid(MonitoringUuid::Uuid).unique_key())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MonitoringUuid::Table).to_owned())
            .await
    }
}
