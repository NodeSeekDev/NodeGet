use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(KvInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(KvInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(KvInDatabase::Namespace).string().not_null())
                    .col(ColumnDef::new(KvInDatabase::Key).string().not_null())
                    .col(ColumnDef::new(KvInDatabase::Value).json_binary().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-kv-namespace-key-unique")
                    .table(KvInDatabase::Table)
                    .col(KvInDatabase::Namespace)
                    .col(KvInDatabase::Key)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-kv-namespace")
                    .table(KvInDatabase::Table)
                    .col(KvInDatabase::Namespace)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(KvInDatabase::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum KvInDatabase {
    #[sea_orm(iden = "kv")]
    Table,
    Id,
    Namespace,
    Key,
    Value,
}
