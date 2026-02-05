use crate::sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

// 迁移名称派生宏
#[derive(DeriveMigrationName)]
pub struct Migration;

// 静态监控数据表的创建和删除迁移实现
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // 执行迁移：创建静态监控表
    // 
    // 该函数创建一个名为 static_monitoring 的表，包含以下列：
    // - id: 主键，自增大整数
    // - uuid: Agent 设备的 UUID
    // - timestamp: 数据记录的时间戳
    // - cpu_data: CPU 静态数据，JSON 格式
    // - system_data: 系统静态数据，JSON 格式
    // - gpu_data: GPU 静态数据，JSON 格式
    // 
    // 还会创建一个复合索引 (uuid, timestamp)，并在 PostgreSQL 上启用 LZ4 压缩
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
                    .table(StaticMonitoringInDatabase::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::Uuid)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::CpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::SystemData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StaticMonitoringInDatabase::GpuData)
                            .json_binary()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-static-uuid-timestamp")
                    .table(StaticMonitoringInDatabase::Table)
                    .col(StaticMonitoringInDatabase::Uuid)
                    .col(StaticMonitoringInDatabase::Timestamp)
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres => {
                let db = manager.get_connection();
                db.execute_unprepared(
                    "ALTER TABLE static_monitoring
                        ALTER COLUMN cpu_data SET COMPRESSION lz4,
                        ALTER COLUMN system_data SET COMPRESSION lz4,
                        ALTER COLUMN gpu_data SET COMPRESSION lz4;",
                )
                .await?;
            }
            DbBackend::Sqlite => {}
            _ => {}
        }
        Ok(())
    }

    // 回滚迁移：删除静态监控表
    // 
    // # 参数
    // * `manager` - 模式管理器
    // 
    // # 返回值
    // 成功时返回 Ok(())，失败时返回数据库错误
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(StaticMonitoringInDatabase::Table)
                    .to_owned(),
            )
            .await
    }
}

// 静态监控表的标识符枚举，用于定义表和列的名称
#[derive(DeriveIden)]
enum StaticMonitoringInDatabase {
    #[sea_orm(iden = "static_monitoring")]
    Table,
    Id,
    Uuid,
    Timestamp,

    CpuData,
    SystemData,
    GpuData,
}
