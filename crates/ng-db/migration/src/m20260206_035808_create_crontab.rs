use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CrontabInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CrontabInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CrontabInDatabase::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(CrontabInDatabase::Enable)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CrontabInDatabase::CronExpression)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CrontabInDatabase::CronType)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CrontabInDatabase::LastRunTime)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-crontab-name") // 索引名称
                    .table(CrontabInDatabase::Table)
                    .col(CrontabInDatabase::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CrontabInDatabase::Table).to_owned())
            .await
    }
}

// 令牌表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum CrontabInDatabase {
    #[sea_orm(iden = "crontab")]
    Table,
    Id,
    Name,
    Enable,
    CronExpression,
    CronType,
    LastRunTime,
}
