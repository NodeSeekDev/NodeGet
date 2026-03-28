use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(JsResultInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(JsResultInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::JsWorkerId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::JsWorkerName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::StartTime)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::FinishTime)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::Param)
                            .json_binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::Result)
                            .json_binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsResultInDatabase::ErrorMessage)
                            .string().null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-js_result-id") // 索引名称
                    .table(JsResultInDatabase::Table)
                    .col(JsResultInDatabase::Id)
                    .unique()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(JsResultInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum JsResultInDatabase {
    #[sea_orm(iden = "js_result")]
    Table,
    Id,
    JsWorkerId,
    JsWorkerName,
    StartTime,
    FinishTime,
    Param,
    Result,
    ErrorMessage,
}
