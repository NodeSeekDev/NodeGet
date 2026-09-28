//! `agent.query_traffic` RPC 实现。
//!
//! 查询一台设备在指定时间段内的流量。
//! 数据来自 `traffic_snapshot`、`traffic_current_total`、`traffic_possible_data_loss` 三张表，
//! 不调用 `TrafficStats`。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{
    DynamicDataQueryField, InterfaceTrafficItem, PossibleDataLossItem, TrafficDetailResponse,
    TrafficGranularity, TrafficQuery, TrafficSnapshotItem, TrafficTotalResponse,
};
use crate::rpc::agent::AgentRpcImpl;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::{traffic_current_total, traffic_possible_data_loss, traffic_snapshot};
use ng_infra::server::{RpcHelper, to_rpc_error};
use ng_token::get::check_token_limit;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};
use serde_json::value::RawValue;
use tracing::debug;

/// `detail` 的最长时间范围（毫秒），92 天
const MAX_DETAIL_RANGE_MS: i64 = 92 * 24 * 60 * 60 * 1000;

/// 查询流量统计。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `granularity` 为 `total` 时返回 `TrafficTotalResponse`，为 `detail` 时返回 `TrafficDetailResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间时返回错误
/// 4. 按 `granularity` 查询流量（`query_total` / `query_detail`）
/// 5. 查询可能丢失数据的时间段（`query_possible_data_losses`），一并返回
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 开始时间晚于结束时间、`detail` 时间范围超过 `MAX_DETAIL_RANGE_MS` 时返回 `NodegetError::InvalidInput`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic(token: String, query: TrafficQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        let is_allowed = check_token_limit(
            &token_or_auth,
            &[Scope::AgentUuid(query.uuid)],
            &[Permission::DynamicMonitoring(DynamicMonitoring::Read(
                DynamicDataQueryField::Network,
            ))],
        )
        .await?;
        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Read(network) permission for this Agent"
                    .to_owned(),
            )
            .into());
        }

        let uuid_id = MonitoringUuidCache::global()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
            })?
            .get_id(&query.uuid)
            .ok_or_else(|| NodegetError::NotFound(format!("Unknown agent UUID: {}", query.uuid)))?;

        if let (Some(start_time), Some(end_time)) = (query.start_time, query.end_time)
            && start_time > end_time
        {
            return Err(NodegetError::InvalidInput(
                "start_time must not be later than end_time".to_owned(),
            )
            .into());
        }

        let db = AgentRpcImpl::get_db()?;
        let possible_data_losses =
            query_possible_data_losses(db, uuid_id, query.start_time, query.end_time).await?;

        let response = match query.granularity {
            TrafficGranularity::Total => {
                let interfaces = query_total(db, uuid_id, query.start_time, query.end_time).await?;
                let received = interfaces.iter().map(|item| item.received).sum();
                let transmitted = interfaces.iter().map(|item| item.transmitted).sum();
                serde_json::value::to_raw_value(&TrafficTotalResponse {
                    uuid: query.uuid,
                    start_time: query.start_time,
                    end_time: query.end_time,
                    interfaces,
                    received,
                    transmitted,
                    possible_data_losses,
                })
            }
            TrafficGranularity::Detail => {
                let snapshots = query_detail(db, uuid_id, query.start_time, query.end_time).await?;
                serde_json::value::to_raw_value(&TrafficDetailResponse {
                    uuid: query.uuid,
                    start_time: query.start_time,
                    end_time: query.end_time,
                    snapshots,
                    possible_data_losses,
                })
            }
        }
        .map_err(|e| NodegetError::SerializationError(format!("traffic response: {e}")))?;

        debug!(target: "monitoring", uuid = %query.uuid, granularity = ?query.granularity, "Traffic query completed");
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
/// 1. 开始值：开始时间之前（含）最近的一条快照；没有快照或未填开始时间时为 0
/// 2. 结束值：未填结束时间时取 `traffic_current_total`；否则取结束时间之前（含）最近的一条快照
/// 3. 流量 = 结束值 − 开始值
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
                .unwrap_or((0, 0)),
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

/// 查询时间段内的所有总流量快照。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 开始时间到结束时间（含两端）之间的快照，按网卡名、快照时间排序
///
/// 1. 时间范围超过 `MAX_DETAIL_RANGE_MS` 时返回错误，未填的一端按最早快照时间、当前时间计算
/// 2. 查询范围内的快照
async fn query_detail(
    db: &DatabaseConnection,
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<TrafficSnapshotItem>> {
    let range_start = match start_time {
        Some(start_time) => Some(start_time),
        None => traffic_snapshot::Entity::find()
            .filter(traffic_snapshot::Column::UuidId.eq(uuid_id))
            .order_by_asc(traffic_snapshot::Column::SnapshotTime)
            .one(db)
            .await
            .map_err(|e| database_error(&e))?
            .map(|earliest| earliest.snapshot_time),
    };
    let Some(range_start) = range_start else {
        // 未填开始时间且没有任何快照
        return Ok(Vec::new());
    };
    let range_end = match end_time {
        Some(end_time) => end_time,
        None => get_local_timestamp_ms_i64()?,
    };
    if range_end - range_start > MAX_DETAIL_RANGE_MS {
        return Err(NodegetError::InvalidInput(
            "detail time range must not exceed 92 days".to_owned(),
        )
        .into());
    }

    let snapshots = traffic_snapshot::Entity::find()
        .filter(traffic_snapshot::Column::UuidId.eq(uuid_id))
        .filter(traffic_snapshot::Column::SnapshotTime.gte(range_start))
        .filter(traffic_snapshot::Column::SnapshotTime.lte(range_end))
        .order_by_asc(traffic_snapshot::Column::InterfaceName)
        .order_by_asc(traffic_snapshot::Column::SnapshotTime)
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    Ok(snapshots
        .into_iter()
        .map(|snapshot| TrafficSnapshotItem {
            interface_name: snapshot.interface_name,
            snapshot_time: snapshot.snapshot_time,
            total_received: snapshot.total_received.cast_unsigned(),
            total_transmitted: snapshot.total_transmitted.cast_unsigned(),
        })
        .collect())
}

/// 查询与时间段有重叠的可能丢失数据的时间段。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 按开始时间排序
async fn query_possible_data_losses(
    db: &DatabaseConnection,
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<PossibleDataLossItem>> {
    let mut select = traffic_possible_data_loss::Entity::find()
        .filter(traffic_possible_data_loss::Column::UuidId.eq(uuid_id));
    // 有重叠：丢失时间段在查询开始之后结束，并且在查询结束之前开始
    if let Some(start_time) = start_time {
        select = select.filter(traffic_possible_data_loss::Column::EndTime.gte(start_time));
    }
    if let Some(end_time) = end_time {
        select = select.filter(traffic_possible_data_loss::Column::StartTime.lte(end_time));
    }

    let losses = select
        .order_by_asc(traffic_possible_data_loss::Column::StartTime)
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    Ok(losses
        .into_iter()
        .map(|loss| PossibleDataLossItem {
            start_time: loss.start_time,
            end_time: loss.end_time,
        })
        .collect())
}

/// 把数据库错误转成 `NodegetError::DatabaseError`。
///
/// - `e`: 数据库错误
fn database_error(e: &DbErr) -> NodegetError {
    NodegetError::DatabaseError(format!("traffic query: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_stats::traffic_tables_on_sqlite;
    use ng_db::entity::{traffic_current_total, traffic_possible_data_loss, traffic_snapshot};
    use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait, Set};

    const MINUTE: i64 = 60 * 1000;
    /// 2026-09-27 10:00:00 UTC
    const TEN_OCLOCK: i64 = 1_790_503_200_000;

    /// 某个时刻：10 点过几分
    const fn at(minutes: i64) -> i64 {
        TEN_OCLOCK + minutes * MINUTE
    }

    /// 准备测试数据：
    /// - eth0：10:00 快照 100，10:15 快照 150，10:30 快照 230，当前总流量 260
    /// - eth1：10:15 快照 10，当前总流量 20
    /// - 可能丢失数据：10:20 到 10:40
    /// - 另一台设备（编号 2）的数据不应被查到
    async fn seeded_db() -> DatabaseConnection {
        let db = traffic_tables_on_sqlite().await;
        let snapshot =
            |uuid_id: i16, name: &str, minutes: i64, total: i64| traffic_snapshot::ActiveModel {
                id: ActiveValue::default(),
                uuid_id: Set(uuid_id),
                interface_name: Set(name.to_owned()),
                snapshot_time: Set(at(minutes)),
                total_received: Set(total),
                total_transmitted: Set(total / 10),
            };
        traffic_snapshot::Entity::insert_many([
            snapshot(1, "eth0", 0, 100),
            snapshot(1, "eth0", 15, 150),
            snapshot(1, "eth0", 30, 230),
            snapshot(1, "eth1", 15, 10),
            snapshot(2, "eth0", 15, 9999),
        ])
        .exec(&db)
        .await
        .unwrap();

        let current_total =
            |uuid_id: i16, name: &str, total: i64| traffic_current_total::ActiveModel {
                id: ActiveValue::default(),
                uuid_id: Set(uuid_id),
                interface_name: Set(name.to_owned()),
                boot_id: Set(None),
                ifindex: Set(None),
                counter_received: Set(0),
                counter_transmitted: Set(0),
                report_time: Set(0),
                total_received: Set(total),
                total_transmitted: Set(total / 10),
                created_at: Set(0),
                updated_at: Set(0),
            };
        traffic_current_total::Entity::insert_many([
            current_total(1, "eth0", 260),
            current_total(1, "eth1", 20),
            current_total(2, "eth0", 9999),
        ])
        .exec(&db)
        .await
        .unwrap();

        traffic_possible_data_loss::Entity::insert(traffic_possible_data_loss::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(1),
            start_time: Set(at(20)),
            end_time: Set(at(40)),
        })
        .exec(&db)
        .await
        .unwrap();
        db
    }

    /// 把合计结果变成 (网卡名, 接收量) 列表，方便比较
    fn received(items: &[crate::query::InterfaceTrafficItem]) -> Vec<(&str, u64)> {
        items
            .iter()
            .map(|item| (item.interface_name.as_str(), item.received))
            .collect()
    }

    #[tokio::test]
    async fn total_between_two_times_uses_latest_snapshots_before_each() {
        let db = seeded_db().await;
        // 开始 10:05 → eth0 取 10:00 的 100，eth1 没有快照取 0
        // 结束 10:31 → eth0 取 10:30 的 230，eth1 取 10:15 的 10
        let items = query_total(&db, 1, Some(at(5)), Some(at(31)))
            .await
            .unwrap();
        assert_eq!(received(&items), [("eth0", 130), ("eth1", 10)]);
        assert_eq!(items[0].transmitted, 13);
    }

    #[tokio::test]
    async fn total_to_now_uses_current_total() {
        let db = seeded_db().await;
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

    #[tokio::test]
    async fn detail_returns_snapshots_in_range_ordered() {
        let db = seeded_db().await;
        let snapshots = query_detail(&db, 1, Some(at(10)), Some(at(30)))
            .await
            .unwrap();
        let listed: Vec<_> = snapshots
            .iter()
            .map(|s| (s.interface_name.as_str(), s.snapshot_time, s.total_received))
            .collect();
        assert_eq!(
            listed,
            [
                ("eth0", at(15), 150),
                ("eth0", at(30), 230),
                ("eth1", at(15), 10)
            ]
        );
    }

    #[tokio::test]
    async fn detail_rejects_range_longer_than_limit() {
        let db = seeded_db().await;
        let result = query_detail(&db, 1, Some(at(0)), Some(at(0) + MAX_DETAIL_RANGE_MS + 1)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn possible_data_losses_are_returned_only_when_overlapping() {
        let db = seeded_db().await;
        let before = query_possible_data_losses(&db, 1, Some(at(0)), Some(at(10)))
            .await
            .unwrap();
        assert!(before.is_empty());

        let overlapping = query_possible_data_losses(&db, 1, Some(at(30)), None)
            .await
            .unwrap();
        assert_eq!(overlapping.len(), 1);
        assert_eq!(overlapping[0].start_time, at(20));
        assert_eq!(overlapping[0].end_time, at(40));
    }
}
