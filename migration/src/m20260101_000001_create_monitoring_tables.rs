use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create static_monitoring table
        manager
            .create_table(
                Table::create()
                    .table(StaticMonitoring::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(StaticMonitoring::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(StaticMonitoring::NodeUuid).uuid().not_null())
                    .col(ColumnDef::new(StaticMonitoring::Time).big_integer().not_null())
                    .col(ColumnDef::new(StaticMonitoring::PhysicalCores).big_integer())
                    .col(ColumnDef::new(StaticMonitoring::LogicalCores).big_integer())
                    .col(ColumnDef::new(StaticMonitoring::SystemName).string())
                    .col(ColumnDef::new(StaticMonitoring::SystemKernel).string())
                    .col(ColumnDef::new(StaticMonitoring::SystemKernelVersion).string())
                    .col(ColumnDef::new(StaticMonitoring::SystemOsVersion).string())
                    .col(ColumnDef::new(StaticMonitoring::SystemOsLongVersion).string())
                    .col(ColumnDef::new(StaticMonitoring::DistributionId).string())
                    .col(ColumnDef::new(StaticMonitoring::SystemHostName).string())
                    .col(ColumnDef::new(StaticMonitoring::Arch).string())
                    .col(ColumnDef::new(StaticMonitoring::Virtualization).string())
                    .to_owned(),
            )
            .await?;

        // Create static_cpu_per_core table
        manager
            .create_table(
                Table::create()
                    .table(StaticCpuPerCore::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(StaticCpuPerCore::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(StaticCpuPerCore::StaticMonitoringId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(StaticCpuPerCore::CoreId).integer().not_null())
                    .col(ColumnDef::new(StaticCpuPerCore::CoreName).string().not_null())
                    .col(ColumnDef::new(StaticCpuPerCore::VendorId).string())
                    .col(ColumnDef::new(StaticCpuPerCore::Brand).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_static_cpu_per_core_static_monitoring_id")
                            .from(StaticCpuPerCore::Table, StaticCpuPerCore::StaticMonitoringId)
                            .to(StaticMonitoring::Table, StaticMonitoring::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create dynamic_monitoring table
        manager
            .create_table(
                Table::create()
                    .table(DynamicMonitoring::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicMonitoring::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DynamicMonitoring::NodeUuid).uuid().not_null())
                    .col(ColumnDef::new(DynamicMonitoring::Time).big_integer().not_null())
                    .col(ColumnDef::new(DynamicMonitoring::TotalCpuUsage).double())
                    .col(ColumnDef::new(DynamicMonitoring::TotalMemory).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::AvailableMemory).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::UsedMemory).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::TotalSwap).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::UsedSwap).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::LoadOne).double())
                    .col(ColumnDef::new(DynamicMonitoring::LoadFive).double())
                    .col(ColumnDef::new(DynamicMonitoring::LoadFifteen).double())
                    .col(ColumnDef::new(DynamicMonitoring::BootTime).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::Uptime).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::ProcessCount).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::UdpConnections).big_integer())
                    .col(ColumnDef::new(DynamicMonitoring::TcpConnections).big_integer())
                    .to_owned(),
            )
            .await?;

        // Create dynamic_cpu_per_core table
        manager
            .create_table(
                Table::create()
                    .table(DynamicCpuPerCore::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicCpuPerCore::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DynamicCpuPerCore::DynamicMonitoringId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DynamicCpuPerCore::CoreId).integer().not_null())
                    .col(ColumnDef::new(DynamicCpuPerCore::CpuUsage).double().not_null())
                    .col(ColumnDef::new(DynamicCpuPerCore::FrequencyMhz).big_integer())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dynamic_cpu_per_core_dynamic_monitoring_id")
                            .from(
                                DynamicCpuPerCore::Table,
                                DynamicCpuPerCore::DynamicMonitoringId,
                            )
                            .to(DynamicMonitoring::Table, DynamicMonitoring::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create dynamic_disk table
        manager
            .create_table(
                Table::create()
                    .table(DynamicDisk::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicDisk::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DynamicDisk::DynamicMonitoringId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicDisk::Kind)
                            .string_len(10)
                            .not_null()
                            .default("Unknown"),
                    )
                    .col(ColumnDef::new(DynamicDisk::Name).string().not_null())
                    .col(ColumnDef::new(DynamicDisk::FileSystem).string())
                    .col(ColumnDef::new(DynamicDisk::MountPoint).string().not_null())
                    .col(ColumnDef::new(DynamicDisk::TotalSpace).big_integer())
                    .col(ColumnDef::new(DynamicDisk::AvailableSpace).big_integer())
                    .col(ColumnDef::new(DynamicDisk::IsRemovable).boolean().not_null())
                    .col(ColumnDef::new(DynamicDisk::IsReadOnly).boolean().not_null())
                    .col(ColumnDef::new(DynamicDisk::ReadSpeed).big_integer())
                    .col(ColumnDef::new(DynamicDisk::WriteSpeed).big_integer())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dynamic_disk_dynamic_monitoring_id")
                            .from(DynamicDisk::Table, DynamicDisk::DynamicMonitoringId)
                            .to(DynamicMonitoring::Table, DynamicMonitoring::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create dynamic_network_interface table
        manager
            .create_table(
                Table::create()
                    .table(DynamicNetworkInterface::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicNetworkInterface::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DynamicNetworkInterface::DynamicMonitoringId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DynamicNetworkInterface::InterfaceName)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DynamicNetworkInterface::TotalReceived).big_integer())
                    .col(ColumnDef::new(DynamicNetworkInterface::TotalTransmitted).big_integer())
                    .col(ColumnDef::new(DynamicNetworkInterface::ReceiveSpeed).big_integer())
                    .col(ColumnDef::new(DynamicNetworkInterface::TransmitSpeed).big_integer())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dynamic_network_interface_dynamic_monitoring_id")
                            .from(
                                DynamicNetworkInterface::Table,
                                DynamicNetworkInterface::DynamicMonitoringId,
                            )
                            .to(DynamicMonitoring::Table, DynamicMonitoring::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DynamicNetworkInterface::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DynamicDisk::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DynamicCpuPerCore::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StaticCpuPerCore::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DynamicMonitoring::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(StaticMonitoring::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum StaticMonitoring {
    Table,
    Id,
    NodeUuid,
    Time,
    PhysicalCores,
    LogicalCores,
    SystemName,
    SystemKernel,
    SystemKernelVersion,
    SystemOsVersion,
    SystemOsLongVersion,
    DistributionId,
    SystemHostName,
    Arch,
    Virtualization,
}

#[derive(DeriveIden)]
enum StaticCpuPerCore {
    Table,
    Id,
    StaticMonitoringId,
    CoreId,
    CoreName,
    VendorId,
    Brand,
}

#[derive(DeriveIden)]
enum DynamicMonitoring {
    Table,
    Id,
    NodeUuid,
    Time,
    TotalCpuUsage,
    TotalMemory,
    AvailableMemory,
    UsedMemory,
    TotalSwap,
    UsedSwap,
    LoadOne,
    LoadFive,
    LoadFifteen,
    BootTime,
    Uptime,
    ProcessCount,
    UdpConnections,
    TcpConnections,
}

#[derive(DeriveIden)]
enum DynamicCpuPerCore {
    Table,
    Id,
    DynamicMonitoringId,
    CoreId,
    CpuUsage,
    FrequencyMhz,
}

#[derive(DeriveIden)]
enum DynamicDisk {
    Table,
    Id,
    DynamicMonitoringId,
    Kind,
    Name,
    FileSystem,
    MountPoint,
    TotalSpace,
    AvailableSpace,
    IsRemovable,
    IsReadOnly,
    ReadSpeed,
    WriteSpeed,
}

#[derive(DeriveIden)]
enum DynamicNetworkInterface {
    Table,
    Id,
    DynamicMonitoringId,
    InterfaceName,
    TotalReceived,
    TotalTransmitted,
    ReceiveSpeed,
    TransmitSpeed,
}