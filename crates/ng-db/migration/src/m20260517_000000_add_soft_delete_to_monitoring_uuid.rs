use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum MonitoringUuid {
    Table,
    SoftDelete,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MonitoringUuid::Table)
                    .add_column(
                        boolean(MonitoringUuid::SoftDelete)
                            .default(false)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MonitoringUuid::Table)
                    .drop_column(MonitoringUuid::SoftDelete)
                    .to_owned(),
            )
            .await
    }
}
