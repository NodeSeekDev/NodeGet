use crate::sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TaskInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TaskInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TaskInDatabase::Uuid).uuid().not_null())
                    .col(ColumnDef::new(TaskInDatabase::Token).string().not_null())
                    .col(ColumnDef::new(TaskInDatabase::CronSource).string().null())
                    .col(
                        ColumnDef::new(TaskInDatabase::Timestamp)
                            .big_integer()
                            .null(), // 任务创建时为 Null，完成后填充
                    )
                    .col(
                        ColumnDef::new(TaskInDatabase::Success)
                            .boolean()
                            .null()
                            .default(false),
                    )
                    .col(ColumnDef::new(TaskInDatabase::ErrorMessage).string().null())
                    .col(
                        ColumnDef::new(TaskInDatabase::TaskEventType)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskInDatabase::TaskEventResult)
                            .json_binary()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-task-uuid-timestamp")
                    .table(TaskInDatabase::Table)
                    .col(TaskInDatabase::Uuid)
                    .col(TaskInDatabase::Timestamp)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-task-cron-source")
                    .table(TaskInDatabase::Table)
                    .col(TaskInDatabase::CronSource)
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres => {
                let db = manager.get_connection();
                db.execute_unprepared(
                    "ALTER TABLE task
                        ALTER COLUMN task_event_result SET COMPRESSION lz4,
                        ALTER COLUMN task_event_type SET COMPRESSION lz4;",
                )
                .await?;
            }
            DbBackend::Sqlite => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TaskInDatabase::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum TaskInDatabase {
    #[sea_orm(iden = "task")]
    Table,
    Id,
    Uuid,
    Token,
    CronSource,
    Timestamp,
    Success,

    ErrorMessage,
    TaskEventType,
    TaskEventResult,
}
