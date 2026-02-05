use crate::sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

// 迁移名称派生宏
#[derive(DeriveMigrationName)]
pub struct Migration;

// 任务数据表的创建和删除迁移实现
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // 执行迁移：创建任务表
    //
    // 该函数创建一个名为 task 的表，包含以下列：
    // - id: 主键，自增大整数
    // - uuid: Agent 设备的 UUID
    // - token: 任务令牌
    // - timestamp: 任务完成时间戳，可为空
    // - success: 任务执行成功状态，可为空
    // - error_message: 错误消息，可为空
    // - task_event_type: 任务事件类型，JSON 格式
    // - task_event_result: 任务事件结果，JSON 格式，可为空
    //
    // 还会创建一个复合索引 (uuid, token)，并在 PostgreSQL 上启用 LZ4 压缩
    //
    // # 参数
    // * `manager` - 模式管理器
    //
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
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
                    .col(ColumnDef::new(TaskInDatabase::Success).boolean().null())
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
                    .name("idx-task-uuid-token")
                    .table(TaskInDatabase::Table)
                    .col(TaskInDatabase::Uuid)
                    .col(TaskInDatabase::Token)
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres => {
                let db = manager.get_connection();
                db.execute_unprepared(
                    "ALTER TABLE task
                        ALTER COLUMN task_event_type SET COMPRESSION lz4,
                        ALTER COLUMN task_event_result SET COMPRESSION lz4;",
                )
                .await?;
            }
            DbBackend::Sqlite => {}
            _ => {}
        }

        Ok(())
    }

    // 回滚迁移：删除任务表
    //
    // # 参数
    // * `manager` - 模式管理器
    //
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TaskInDatabase::Table).to_owned())
            .await
    }
}

// 任务表的标识符枚举，用于定义表和列的名称
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
