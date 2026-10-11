//! `agent.query_traffic_total` RPC 实现。
//!
//! 查询一台设备在指定时间段内的流量合计。
//! 数据来自 `traffic_snapshot`、`traffic_current_total`、`traffic_possible_data_loss` 三张表，
//! 不调用 `TrafficStats`。

use crate::query::{InterfaceTrafficItem, TrafficTotalQuery, TrafficTotalResponse};
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::query_traffic_common::{
    authorize_and_resolve, check_time_order, database_error, query_possible_data_losses,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_db::entity::{traffic_current_total, traffic_snapshot};
use ng_infra::server::{RpcHelper, to_rpc_error};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
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
/// - 填了开始时间但某块网卡找不到起点快照时返回 `NodegetError::NotFound`
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

/// 查询时间段内每块网卡的流量合计。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 每块网卡的流量，按网卡名排序；结束时间之前还没有数据的网卡不返回
///
/// 每块网卡分别计算：
/// 1. 结束值：未填结束时间时取 `traffic_current_total`；否则取结束时间之前（含）最近的一条快照，
///    没有则这块网卡不返回
/// 2. 开始值：未填开始时间时为 0；否则取开始时间之前（含）最近的一条快照
/// 3. 流量 = 结束值 − 开始值
///
/// 填了开始时间但某块网卡找不到起点快照时返回错误：没有快照就不知道开始时的总流量，
/// 不能当 0 算，否则会把开始之前的流量都算进去。快照由 Worker 定时写入，
/// 没装 Worker、Worker 没运行，或设备在开始时间之后才开始统计，都会这样。
///
/// # Errors
///
/// - 填了开始时间但某块网卡找不到起点快照时返回 `NodegetError::NotFound`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
async fn query_total(
    db: &DatabaseConnection,
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<InterfaceTrafficItem>> {
    let current_totals = traffic_current_total::Entity::find()
        .filter(traffic_current_total::Column::UuidId.eq(uuid_id))
        .order_by_asc(traffic_current_total::Column::InterfaceName)
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    let mut items = Vec::with_capacity(current_totals.len());
    for current_total in current_totals {
        let interface_name = current_total.interface_name;
        let end_value = match end_time {
            None => Some((
                current_total.total_received,
                current_total.total_transmitted,
            )),
            Some(end_time) => latest_snapshot_at(db, uuid_id, &interface_name, end_time).await?,
        };
        let Some((end_received, end_transmitted)) = end_value else {
            continue;
        };
        let (start_received, start_transmitted) = match start_time {
            None => (0, 0),
            Some(start_time) => latest_snapshot_at(db, uuid_id, &interface_name, start_time)
                .await?
                .ok_or_else(|| {
                    NodegetError::NotFound(format!(
                        "No traffic snapshot of interface {interface_name} at or before start_time; \
                         make sure the traffic snapshot worker is installed and running \
                         (traffic before the agent was installed is not counted)"
                    ))
                })?,
        };

        items.push(InterfaceTrafficItem {
            interface_name,
            received: end_received.saturating_sub(start_received).cast_unsigned(),
            transmitted: end_transmitted
                .saturating_sub(start_transmitted)
                .cast_unsigned(),
        });
    }
    Ok(items)
}

/// 查询某块网卡在某个时刻之前（含）最近的一条快照。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `interface_name`: 网卡名
/// - `time`: 时刻（毫秒）
/// - 返回: 快照的（总接收量, 总发送量），没有快照时为 `None`
async fn latest_snapshot_at(
    db: &DatabaseConnection,
    uuid_id: i16,
    interface_name: &str,
    time: i64,
) -> anyhow::Result<Option<(i64, i64)>> {
    let snapshot = traffic_snapshot::Entity::find()
        .filter(traffic_snapshot::Column::UuidId.eq(uuid_id))
        .filter(traffic_snapshot::Column::InterfaceName.eq(interface_name))
        .filter(traffic_snapshot::Column::SnapshotTime.lte(time))
        .order_by_desc(traffic_snapshot::Column::SnapshotTime)
        .one(db)
        .await
        .map_err(|e| database_error(&e))?;
    Ok(snapshot.map(|snapshot| (snapshot.total_received, snapshot.total_transmitted)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::agent::query_traffic_common::test_support::{at, seeded_db};

    /// 把合计结果变成 (网卡名, 接收量) 列表，方便比较
    fn received(items: &[InterfaceTrafficItem]) -> Vec<(&str, u64)> {
        items
            .iter()
            .map(|item| (item.interface_name.as_str(), item.received))
            .collect()
    }

    #[tokio::test]
    async fn total_between_two_times_uses_latest_snapshots_before_each() {
        let db = seeded_db().await;
        // 开始 10:20 → eth0 取 10:15 的 150，eth1 取 10:15 的 10
        // 结束 10:31 → eth0 取 10:30 的 230，eth1 取 10:15 的 10
        let items = query_total(&db, 1, Some(at(20)), Some(at(31)))
            .await
            .unwrap();
        assert_eq!(received(&items), [("eth0", 80), ("eth1", 0)]);
        assert_eq!(items[0].transmitted, 8);
    }

    #[tokio::test]
    async fn total_without_start_snapshot_reports_not_found() {
        let db = seeded_db().await;
        // 开始 10:05：eth0 有 10:00 的快照，但 eth1 最早的快照是 10:15，找不到起点
        let error = query_total(&db, 1, Some(at(5)), Some(at(31)))
            .await
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NodegetError>(),
            Some(NodegetError::NotFound(_))
        ));

        // 开始时间早于所有快照同样报缺失，不能当 0 算
        let error = query_total(&db, 1, Some(at(-60)), None).await.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NodegetError>(),
            Some(NodegetError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn total_to_now_uses_current_total() {
        let db = seeded_db().await;
        // 开始 10:15 → eth0 取 150，eth1 取 10；到现在 eth0 260，eth1 20
        let items = query_total(&db, 1, Some(at(15)), None).await.unwrap();
        assert_eq!(received(&items), [("eth0", 110), ("eth1", 10)]);

        let items = query_total(&db, 1, None, None).await.unwrap();
        assert_eq!(received(&items), [("eth0", 260), ("eth1", 20)]);
    }

    #[tokio::test]
    async fn total_before_any_snapshot_returns_no_interfaces() {
        let db = seeded_db().await;
        let items = query_total(&db, 1, None, Some(at(-60))).await.unwrap();
        assert!(items.is_empty());
    }
}
