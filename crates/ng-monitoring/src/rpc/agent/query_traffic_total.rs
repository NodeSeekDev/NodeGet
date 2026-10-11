//! `agent.query_traffic_total` RPC 实现。
//!
//! 查询一台设备在指定时间段内的流量合计。
//! 数据来自 `traffic_snapshot`、`traffic_possible_data_loss` 两张表，不调用 `TrafficStats`。

use crate::query::{InterfaceTrafficItem, TrafficTotalQuery, TrafficTotalResponse};
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::query_traffic_common::{
    authorize_and_resolve, check_time_order, database_error, query_possible_data_losses,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

/// 查询一段时间内每块网卡的流量合计。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `TrafficTotalResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间时返回错误
/// 4. 查询每块网卡的流量合计（`query_total`），以及可能丢失数据的时间段（`query_possible_data_losses`）
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 时间段内没有一块网卡有至少两条快照时返回 `NodegetError::NotFound`
/// - 开始时间晚于结束时间时返回 `NodegetError::InvalidInput`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic_total(
    token: String,
    query: TrafficTotalQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let uuid_id = authorize_and_resolve(&token, query.uuid).await?;
        check_time_order(query.start_time, query.end_time)?;

        let db = AgentRpcImpl::get_db()?;
        let possible_data_losses =
            query_possible_data_losses(db, uuid_id, query.start_time, query.end_time).await?;
        let interfaces = query_total(db, uuid_id, query.start_time, query.end_time).await?;
        let received = interfaces.iter().map(|item| item.received).sum();
        let transmitted = interfaces.iter().map(|item| item.transmitted).sum();
        let response = serde_json::value::to_raw_value(&TrafficTotalResponse {
            uuid: query.uuid,
            start_time: query.start_time,
            end_time: query.end_time,
            interfaces,
            received,
            transmitted,
            possible_data_losses,
        })
        .map_err(|e| NodegetError::SerializationError(format!("traffic response: {e}")))?;

        debug!(target: "monitoring", uuid = %query.uuid, "Traffic total query completed");
        Ok(response)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// 一块网卡在时间段内的快照汇总：
/// （网卡名, 快照条数, 最早快照时间, 最晚快照时间, 最小总接收量, 最大总接收量, 最小总发送量, 最大总发送量）
type SnapshotSummary = (String, i64, i64, i64, i64, i64, i64, i64);

/// 查询时间段内每块网卡的流量合计。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 每块网卡的流量，按网卡名排序
///
/// 只用开始时间到结束时间（含两端）之间的快照，不往时间段外找。每块网卡分别计算：
/// 总流量只增不减，所以流量 = 时间段内最大的总流量 − 最小的总流量，
/// 也就是最后一条快照减第一条快照。同时返回这两条快照的时间。
/// 时间段内只有一条快照的网卡没法相减，不返回。
///
/// # Errors
///
/// - 没有一块网卡有至少两条快照时返回 `NodegetError::NotFound`，不返回空结果，避免被当成 0 流量
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
async fn query_total(
    db: &DatabaseConnection,
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<InterfaceTrafficItem>> {
    let mut select = traffic_snapshot::Entity::find()
        .select_only()
        .column(traffic_snapshot::Column::InterfaceName)
        .column_as(traffic_snapshot::Column::Id.count(), "snapshot_count")
        .column_as(
            traffic_snapshot::Column::SnapshotTime.min(),
            "first_snapshot_time",
        )
        .column_as(
            traffic_snapshot::Column::SnapshotTime.max(),
            "last_snapshot_time",
        )
        .column_as(
            traffic_snapshot::Column::TotalReceived.min(),
            "min_received",
        )
        .column_as(
            traffic_snapshot::Column::TotalReceived.max(),
            "max_received",
        )
        .column_as(
            traffic_snapshot::Column::TotalTransmitted.min(),
            "min_transmitted",
        )
        .column_as(
            traffic_snapshot::Column::TotalTransmitted.max(),
            "max_transmitted",
        )
        .filter(traffic_snapshot::Column::UuidId.eq(uuid_id));
    if let Some(start_time) = start_time {
        select = select.filter(traffic_snapshot::Column::SnapshotTime.gte(start_time));
    }
    if let Some(end_time) = end_time {
        select = select.filter(traffic_snapshot::Column::SnapshotTime.lte(end_time));
    }
    let rows: Vec<SnapshotSummary> = select
        .group_by(traffic_snapshot::Column::InterfaceName)
        .order_by_asc(traffic_snapshot::Column::InterfaceName)
        .into_tuple()
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    let items: Vec<_> = rows
        .into_iter()
        .filter(|row| row.1 >= 2)
        .map(
            |(
                interface_name,
                _,
                first_snapshot_time,
                last_snapshot_time,
                min_received,
                max_received,
                min_transmitted,
                max_transmitted,
            )| InterfaceTrafficItem {
                interface_name,
                received: max_received.saturating_sub(min_received).cast_unsigned(),
                transmitted: max_transmitted
                    .saturating_sub(min_transmitted)
                    .cast_unsigned(),
                first_snapshot_time,
                last_snapshot_time,
            },
        )
        .collect();
    if items.is_empty() {
        return Err(NodegetError::NotFound(
            "No interface has at least two traffic snapshots between start_time and end_time; \
             make sure the traffic snapshot worker is installed and running, \
             or query the snapshots with query_traffic_detail"
                .to_owned(),
        )
        .into());
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::agent::query_traffic_common::test_support::{at, seeded_db};
    use sea_orm::{ActiveValue, Set};

    /// 把合计结果变成 (网卡名, 接收量) 列表，方便比较
    fn received(items: &[InterfaceTrafficItem]) -> Vec<(&str, u64)> {
        items
            .iter()
            .map(|item| (item.interface_name.as_str(), item.received))
            .collect()
    }

    /// 断言查询返回 `NodegetError::NotFound`。
    fn assert_not_found(result: anyhow::Result<Vec<InterfaceTrafficItem>>) {
        let error = result.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NodegetError>(),
            Some(NodegetError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn total_uses_first_and_last_snapshot_inside_the_range() {
        let db = seeded_db().await;
        // 10:10 到 10:30 之间 eth0 有 10:15 的 150 和 10:30 的 230；
        // eth1 只有 10:15 一条，没法相减，不返回
        let items = query_total(&db, 1, Some(at(10)), Some(at(30)))
            .await
            .unwrap();
        assert_eq!(received(&items), [("eth0", 80)]);
        assert_eq!(items[0].transmitted, 8);
        assert_eq!(items[0].first_snapshot_time, at(15));
        assert_eq!(items[0].last_snapshot_time, at(30));
    }

    #[tokio::test]
    async fn total_without_times_covers_all_snapshots() {
        let db = seeded_db().await;
        // 不填时间：eth0 从 10:00 的 100 到 10:30 的 230
        let items = query_total(&db, 1, None, None).await.unwrap();
        assert_eq!(received(&items), [("eth0", 130)]);
        assert_eq!(items[0].first_snapshot_time, at(0));
        assert_eq!(items[0].last_snapshot_time, at(30));
    }

    #[tokio::test]
    async fn total_does_not_look_outside_the_range() {
        let db = seeded_db().await;
        // 开始 10:05：区间内 eth0 只有 10:15 和 10:30 两条，不会去用区间外 10:00 的快照
        let items = query_total(&db, 1, Some(at(5)), None).await.unwrap();
        assert_eq!(received(&items), [("eth0", 80)]);
        assert_eq!(items[0].first_snapshot_time, at(15));
    }

    #[tokio::test]
    async fn total_sums_each_interface_separately() {
        let db = seeded_db().await;
        // 给 eth1 补一条 10:30 的快照，两块网卡都有两条以上
        traffic_snapshot::Entity::insert(traffic_snapshot::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(1),
            interface_name: Set("eth1".to_owned()),
            snapshot_time: Set(at(30)),
            total_received: Set(40),
            total_transmitted: Set(4),
        })
        .exec(&db)
        .await
        .unwrap();

        let items = query_total(&db, 1, None, None).await.unwrap();
        assert_eq!(received(&items), [("eth0", 130), ("eth1", 30)]);
        assert_eq!(items[1].first_snapshot_time, at(15));
        assert_eq!(items[1].last_snapshot_time, at(30));
    }

    #[tokio::test]
    async fn total_without_enough_snapshots_reports_not_found() {
        let db = seeded_db().await;
        // 区间内没有快照
        assert_not_found(query_total(&db, 1, Some(at(16)), Some(at(29))).await);
        // 区间内只有一条快照（eth0 的 10:30），没法相减
        assert_not_found(query_total(&db, 1, Some(at(20)), Some(at(40))).await);
        // 设备没有任何快照
        assert_not_found(query_total(&db, 3, None, None).await);
    }
}
