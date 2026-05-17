use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Static::Table)
                    .add_column(
                        ColumnDef::new(Static::Enable)
                            .boolean()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Static::Table)
                    .drop_column(Static::Enable)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Static {
    #[sea_orm(iden = "static")]
    Table,
    Enable,
}
