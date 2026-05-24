use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DbRegistry::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DbRegistry::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DbRegistry::Name).string().not_null())
                    .col(
                        ColumnDef::new(DbRegistry::DbConnections)
                            .integer()
                            .null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DbRegistry::MaxLifetimeMs)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(DbRegistry::CreatedAt)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-db-registry-name-unique")
                    .table(DbRegistry::Table)
                    .col(DbRegistry::Name)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DbRegistry::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum DbRegistry {
    #[sea_orm(iden = "db_registry")]
    Table,
    Id,
    Name,
    DbConnections,
    MaxLifetimeMs,
    CreatedAt,
}
