use sea_orm_migration::{prelude::*, schema::*};

/// 新增流量统计相关的三张表。
///
/// - `traffic_snapshot`: 每块出口网卡每 15 分钟的总流量快照，供流量查询使用
/// - `traffic_current_total`: 每块出口网卡当前的总流量和上一次的读数，服务端重启后据此继续计算
/// - `traffic_possible_data_loss`: 可能丢失数据的时间段
#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum TrafficSnapshot {
    Table,
    Id,
    UuidId,
    InterfaceName,
    SnapshotTime,
    TotalReceived,
    TotalTransmitted,
}

#[derive(DeriveIden)]
enum TrafficCurrentTotal {
    Table,
    Id,
    UuidId,
    InterfaceName,
    BootId,
    Ifindex,
    CounterReceived,
    CounterTransmitted,
    ReportTime,
    TotalReceived,
    TotalTransmitted,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TrafficPossibleDataLoss {
    Table,
    Id,
    UuidId,
    StartTime,
    EndTime,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // traffic_snapshot
        manager
            .create_table(
                Table::create()
                    .table(TrafficSnapshot::Table)
                    .if_not_exists()
                    .col(
                        big_integer(TrafficSnapshot::Id)
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(small_integer(TrafficSnapshot::UuidId))
                    .col(string(TrafficSnapshot::InterfaceName))
                    .col(big_integer(TrafficSnapshot::SnapshotTime))
                    .col(big_integer(TrafficSnapshot::TotalReceived))
                    .col(big_integer(TrafficSnapshot::TotalTransmitted))
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一 (设备, 网卡, 快照时间) 只有一条；按设备和时间范围查询也走这个索引
        // 索引名不带 -unique 后缀：PostgreSQL 标识符上限 63 字符，超出会被截断
        manager
            .create_index(
                Index::create()
                    .name("idx-traffic_snapshot-uuid_id-interface_name-snapshot_time")
                    .table(TrafficSnapshot::Table)
                    .col(TrafficSnapshot::UuidId)
                    .col(TrafficSnapshot::InterfaceName)
                    .col(TrafficSnapshot::SnapshotTime)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // traffic_current_total
        manager
            .create_table(
                Table::create()
                    .table(TrafficCurrentTotal::Table)
                    .if_not_exists()
                    .col(
                        big_integer(TrafficCurrentTotal::Id)
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(small_integer(TrafficCurrentTotal::UuidId))
                    .col(string(TrafficCurrentTotal::InterfaceName))
                    .col(string_null(TrafficCurrentTotal::BootId))
                    .col(integer_null(TrafficCurrentTotal::Ifindex))
                    .col(big_integer(TrafficCurrentTotal::CounterReceived))
                    .col(big_integer(TrafficCurrentTotal::CounterTransmitted))
                    .col(big_integer(TrafficCurrentTotal::ReportTime))
                    .col(big_integer(TrafficCurrentTotal::TotalReceived))
                    .col(big_integer(TrafficCurrentTotal::TotalTransmitted))
                    .col(big_integer(TrafficCurrentTotal::CreatedAt))
                    .col(big_integer(TrafficCurrentTotal::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        // 每块网卡一行，写库时已存在则更新
        manager
            .create_index(
                Index::create()
                    .name("idx-traffic_current_total-uuid_id-interface_name-unique")
                    .table(TrafficCurrentTotal::Table)
                    .col(TrafficCurrentTotal::UuidId)
                    .col(TrafficCurrentTotal::InterfaceName)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // traffic_possible_data_loss
        manager
            .create_table(
                Table::create()
                    .table(TrafficPossibleDataLoss::Table)
                    .if_not_exists()
                    .col(
                        big_integer(TrafficPossibleDataLoss::Id)
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(small_integer(TrafficPossibleDataLoss::UuidId))
                    .col(big_integer(TrafficPossibleDataLoss::StartTime))
                    .col(big_integer(TrafficPossibleDataLoss::EndTime))
                    .to_owned(),
            )
            .await?;

        // 按设备和时间范围查询
        manager
            .create_index(
                Index::create()
                    .name("idx-traffic_possible_data_loss-uuid_id-start_time")
                    .table(TrafficPossibleDataLoss::Table)
                    .col(TrafficPossibleDataLoss::UuidId)
                    .col(TrafficPossibleDataLoss::StartTime)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(TrafficPossibleDataLoss::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(TrafficCurrentTotal::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TrafficSnapshot::Table).to_owned())
            .await
    }
}
