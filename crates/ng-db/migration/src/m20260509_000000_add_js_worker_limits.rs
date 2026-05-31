use sea_orm_migration::prelude::*;

/// 给 `js_worker` 表新增三列运行时限制：
/// - `max_run_time`        单位毫秒；从 spawn 到返回值的总墙上时钟上限
/// - `max_stack_size`      单位字节；QuickJS C 栈上限（递归深度）
/// - `max_heap_size`       单位字节；QuickJS 堆上限（`rt.set_memory_limit`）
///
/// 三列均可为 NULL；NULL 在应用层兜底为 30_000ms / 1 MiB / 8 MiB。
///
/// 实现细节：SQLite 的 `ALTER TABLE` 一条语句只能加一列
/// （sea-query 对多 option 会 panic），这里逐列发三条 ALTER 以同时
/// 兼容 SQLite 和 PostgreSQL。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for col in [
            JsWorkerInDatabase::MaxRunTime,
            JsWorkerInDatabase::MaxStackSize,
            JsWorkerInDatabase::MaxHeapSize,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(JsWorkerInDatabase::Table)
                        .add_column(ColumnDef::new(col).big_integer().null())
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for col in [
            JsWorkerInDatabase::MaxHeapSize,
            JsWorkerInDatabase::MaxStackSize,
            JsWorkerInDatabase::MaxRunTime,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(JsWorkerInDatabase::Table)
                        .drop_column(col)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden, Clone, Copy)]
enum JsWorkerInDatabase {
    #[sea_orm(iden = "js_worker")]
    Table,
    MaxRunTime,
    MaxStackSize,
    MaxHeapSize,
}
