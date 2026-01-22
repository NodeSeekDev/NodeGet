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
                    .col(
                        ColumnDef::new(TaskInDatabase::Timestamp)
                            .big_integer()
                            .null(),
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
            DbBackend::Sqlite => {} // todo!()
            _ => {
                todo!()
            }
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
    Timestamp,
    Success,

    ErrorMessage,
    TaskEventType,
    TaskEventResult,
}
