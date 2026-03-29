use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(JsWorkerInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::Name)
                            .string()
                            .unique_key()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::JsScript)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::JsByteCode)
                            .binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::RouteName)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::Env)
                            .json_binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::RuntimeCleanTime)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::CreateAt)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(JsWorkerInDatabase::UpdateAt)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-js_worker-id") // 索引名称
                    .table(JsWorkerInDatabase::Table)
                    .col(JsWorkerInDatabase::Id)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-js_worker-route_name-unique")
                    .table(JsWorkerInDatabase::Table)
                    .col(JsWorkerInDatabase::RouteName)
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
                    .table(JsWorkerInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum JsWorkerInDatabase {
    #[sea_orm(iden = "js_worker")]
    Table,

    Id,
    Name,
    JsScript,
    JsByteCode,
    RouteName,

    Env,
    RuntimeCleanTime,

    CreateAt,
    UpdateAt,
}
