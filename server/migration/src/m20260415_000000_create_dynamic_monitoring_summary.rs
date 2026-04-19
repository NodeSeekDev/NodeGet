use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum DynamicMonitoringSummary {
    Table,
    Id,
    UuidId,
    Timestamp,
    CpuUsage,
    GpuUsage,
    UsedSwap,
    TotalSwap,
    UsedMemory,
    TotalMemory,
    AvailableMemory,
    LoadOne,
    LoadFive,
    LoadFifteen,
    Uptime,
    BootTime,
    ProcessCount,
    TotalSpace,
    AvailableSpace,
    ReadSpeed,
    WriteSpeed,
    TcpConnections,
    UdpConnections,
    TotalReceived,
    TotalTransmitted,
    TransmitSpeed,
    ReceiveSpeed,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DynamicMonitoringSummary::Table)
                    .if_not_exists()
                    .col(big_integer(DynamicMonitoringSummary::Id).auto_increment().primary_key())
                    .col(small_integer(DynamicMonitoringSummary::UuidId))
                    .col(big_integer(DynamicMonitoringSummary::Timestamp))
                    .col(small_integer_null(DynamicMonitoringSummary::CpuUsage))
                    .col(small_integer_null(DynamicMonitoringSummary::GpuUsage))
                    .col(big_integer_null(DynamicMonitoringSummary::UsedSwap))
                    .col(big_integer_null(DynamicMonitoringSummary::TotalSwap))
                    .col(big_integer_null(DynamicMonitoringSummary::UsedMemory))
                    .col(big_integer_null(DynamicMonitoringSummary::TotalMemory))
                    .col(big_integer_null(DynamicMonitoringSummary::AvailableMemory))
                    .col(small_integer_null(DynamicMonitoringSummary::LoadOne))
                    .col(small_integer_null(DynamicMonitoringSummary::LoadFive))
                    .col(small_integer_null(DynamicMonitoringSummary::LoadFifteen))
                    .col(integer_null(DynamicMonitoringSummary::Uptime))
                    .col(big_integer_null(DynamicMonitoringSummary::BootTime))
                    .col(integer_null(DynamicMonitoringSummary::ProcessCount))
                    .col(big_integer_null(DynamicMonitoringSummary::TotalSpace))
                    .col(big_integer_null(DynamicMonitoringSummary::AvailableSpace))
                    .col(big_integer_null(DynamicMonitoringSummary::ReadSpeed))
                    .col(big_integer_null(DynamicMonitoringSummary::WriteSpeed))
                    .col(integer_null(DynamicMonitoringSummary::TcpConnections))
                    .col(integer_null(DynamicMonitoringSummary::UdpConnections))
                    .col(big_integer_null(DynamicMonitoringSummary::TotalReceived))
                    .col(big_integer_null(DynamicMonitoringSummary::TotalTransmitted))
                    .col(big_integer_null(DynamicMonitoringSummary::TransmitSpeed))
                    .col(big_integer_null(DynamicMonitoringSummary::ReceiveSpeed))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dynamic_monitoring_summary_uuid_timestamp")
                    .table(DynamicMonitoringSummary::Table)
                    .col(DynamicMonitoringSummary::UuidId)
                    .col(DynamicMonitoringSummary::Timestamp)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(DynamicMonitoringSummary::Table)
                    .to_owned(),
            )
            .await
    }
}
