use sea_orm_migration::prelude::*;

/// 给三张监控表新增 `storage_time` 列，用于记录数据入库时间（毫秒级时间戳）。
/// 与 `timestamp`（Agent 上报时间）不同，`storage_time` 由 Server 在写入时自动生成。
///
/// 新增列均为 NULLABLE，兼容存量数据。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // static_monitoring
        manager
            .alter_table(
                Table::alter()
                    .table(StaticMonitoring::Table)
                    .add_column(
                        ColumnDef::new(StaticMonitoring::StorageTime)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // dynamic_monitoring
        manager
            .alter_table(
                Table::alter()
                    .table(DynamicMonitoring::Table)
                    .add_column(
                        ColumnDef::new(DynamicMonitoring::StorageTime)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // dynamic_monitoring_summary
        manager
            .alter_table(
                Table::alter()
                    .table(DynamicMonitoringSummary::Table)
                    .add_column(
                        ColumnDef::new(DynamicMonitoringSummary::StorageTime)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 严格按照相反顺序 drop，避免外键依赖问题（虽然监控表无显式外键）
        manager
            .alter_table(
                Table::alter()
                    .table(DynamicMonitoringSummary::Table)
                    .drop_column(DynamicMonitoringSummary::StorageTime)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DynamicMonitoring::Table)
                    .drop_column(DynamicMonitoring::StorageTime)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(StaticMonitoring::Table)
                    .drop_column(StaticMonitoring::StorageTime)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum StaticMonitoring {
    #[sea_orm(iden = "static_monitoring")]
    Table,
    StorageTime,
}

#[derive(DeriveIden)]
enum DynamicMonitoring {
    #[sea_orm(iden = "dynamic_monitoring")]
    Table,
    StorageTime,
}

#[derive(DeriveIden)]
enum DynamicMonitoringSummary {
    #[sea_orm(iden = "dynamic_monitoring_summary")]
    Table,
    StorageTime,
}
