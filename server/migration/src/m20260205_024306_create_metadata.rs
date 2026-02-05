use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MetadataInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MetadataInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MetadataInDatabase::Uuid).uuid().not_null())
                    .col(
                        ColumnDef::new(MetadataInDatabase::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(MetadataInDatabase::Tags)
                            .json_binary()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-metadata-uuid") // 索引名称
                    .table(MetadataInDatabase::Table)
                    .col(MetadataInDatabase::Uuid)
                    .unique()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MetadataInDatabase::Table).to_owned())
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum MetadataInDatabase {
    #[sea_orm(iden = "metadata")]
    Table,
    Id,
    Uuid,
    Name,
    Tags,
}
