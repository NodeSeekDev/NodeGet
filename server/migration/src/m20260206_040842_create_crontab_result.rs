use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CrontabResultInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::CronId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::CronName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::TaskId)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::RunTime)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::Success)
                            .boolean()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CrontabResultInDatabase::Message)
                            .string()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-crontab_result-id") // 索引名称
                    .table(CrontabResultInDatabase::Table)
                    .col(CrontabResultInDatabase::Id)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-crontab_result-task_id")
                    .table(CrontabResultInDatabase::Table)
                    .col(CrontabResultInDatabase::TaskId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CrontabResultInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum CrontabResultInDatabase {
    #[sea_orm(iden = "crontab_result")]
    Table,
    Id,
    CronId,
    CronName,
    TaskId,
    RunTime,
    Success,
    Message,
}
