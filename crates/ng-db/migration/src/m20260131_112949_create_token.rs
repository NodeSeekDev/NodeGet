use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TokenInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TokenInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::Version)
                            .integer()
                            .not_null()
                            .default(1)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenKey)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenHash)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TimeStampFrom)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TimeStampTo)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TokenInDatabase::TokenLimit)
                            .json_binary()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TokenInDatabase::Username).string().null())
                    .col(
                        ColumnDef::new(TokenInDatabase::PasswordHash)
                            .string()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-token-key")
                    .table(TokenInDatabase::Table)
                    .col(TokenInDatabase::TokenKey)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-token-username")
                    .table(TokenInDatabase::Table)
                    .col(TokenInDatabase::Username)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TokenInDatabase::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum TokenInDatabase {
    #[sea_orm(iden = "token")]
    Table,
    Id,
    Version,
    TokenKey,
    TokenHash,
    TimeStampFrom,
    TimeStampTo,
    TokenLimit,

    Username,
    PasswordHash,
}
