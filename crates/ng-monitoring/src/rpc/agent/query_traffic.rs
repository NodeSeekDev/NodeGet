//! `agent.query_traffic` RPC 实现。
//!
//! 查询一台设备在指定时间段内的流量。
//! 数据来自 `traffic_snapshot`、`traffic_current_total`、`traffic_possible_data_loss` 三张表，
//! 不调用 `TrafficStats`。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{
    DynamicDataQueryField, InterfaceSnapshotRangeItem, InterfaceTrafficItem, PossibleDataLossItem,
    TrafficDetailResponse, TrafficGranularity, TrafficQuery, TrafficRangeResponse,
    TrafficSnapshotItem, TrafficTotalResponse,
};
use crate::rpc::agent::AgentRpcImpl;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::{DEFAULT_MONITORING_QUERY_LIMIT, get_local_timestamp_ms_i64};
use ng_db::entity::{traffic_current_total, traffic_possible_data_loss, traffic_snapshot};
use ng_infra::server::{RpcHelper, to_rpc_error};
use ng_token::get::check_token_limit;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;
use tracing::debug;

/// `detail` 的最长时间范围（毫秒），92 天
const MAX_DETAIL_RANGE_MS: i64 = 92 * 24 * 60 * 60 * 1000;

/// 查询流量统计。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `granularity` 为 `total` 时返回 `TrafficTotalResponse`，为 `detail` 时返回
///   `TrafficDetailResponse`，为 `range` 时返回 `TrafficRangeResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间时返回错误
/// 4. 按 `granularity` 查询流量（`query_total` / `query_detail` / `query_range`）
/// 5. `total` 和 `detail` 还查询可能丢失数据的时间段（`query_possible_data_losses`），一并返回；
///    `range` 忽略开始时间和结束时间，不返回这一项
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - `total` 填了开始时间但某块网卡找不到起点快照时返回 `NodegetError::NotFound`
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
        let response = match query.granularity {
            TrafficGranularity::Total => {
                let possible_data_losses =
                    query_possible_data_losses(db, uuid_id, query.start_time, query.end_time)
                        .await?;
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
                let possible_data_losses =
                    query_possible_data_losses(db, uuid_id, query.start_time, query.end_time)
                        .await?;
                let snapshots = query_detail(db, uuid_id, query.start_time, query.end_time).await?;
                serde_json::value::to_raw_value(&TrafficDetailResponse {
                    uuid: query.uuid,
                    start_time: query.start_time,
                    end_time: query.end_time,
                    snapshots,
                    possible_data_losses,
                })
            }
            TrafficGranularity::Range => {
                let interfaces = query_range(db, uuid_id).await?;
                serde_json::value::to_raw_value(&TrafficRangeResponse {
                    uuid: query.uuid,
                    interfaces,
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

/// 查询每块网卡有快照数据的时间范围。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - 返回: 每块有快照的网卡最早和最晚的快照时间，按网卡名排序
async fn query_range(
    db: &DatabaseConnection,
    uuid_id: i16,
) -> anyhow::Result<Vec<InterfaceSnapshotRangeItem>> {
    let rows: Vec<(String, i64, i64)> = traffic_snapshot::Entity::find()
        .select_only()
        .column(traffic_snapshot::Column::InterfaceName)
        .column_as(
            traffic_snapshot::Column::SnapshotTime.min(),
            "first_snapshot_time",
        )
        .column_as(
            traffic_snapshot::Column::SnapshotTime.max(),
            "last_snapshot_time",
        )
        .filter(traffic_snapshot::Column::UuidId.eq(uuid_id))
        .group_by(traffic_snapshot::Column::InterfaceName)
        .order_by_asc(traffic_snapshot::Column::InterfaceName)
        .into_tuple()
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    Ok(rows
        .into_iter()
        .map(
            |(interface_name, first_snapshot_time, last_snapshot_time)| {
                InterfaceSnapshotRangeItem {
                    interface_name,
                    first_snapshot_time,
                    last_snapshot_time,
                }
            },
        )
        .collect())
}

/// 判断 `start` 到 `end` 的跨度是否非负且不超过 `max_range_ms`。
///
/// - `start`: 开始时间（毫秒时间戳）
/// - `end`: 结束时间（毫秒时间戳）
/// - `max_range_ms`: 允许的最长跨度（毫秒）
/// - 返回: 跨度非负且不超过 `max_range_ms` 时为 `true`
///
/// 用 `checked_sub` 而不是裸减法：`start = i64::MIN`、`end = i64::MAX` 这类极端取值会让
/// `end - start` 溢出。dev 构建直接 panic，release/minimal 构建（`overflow-checks = false`）
/// 会回绕成负数，使 `> max_range_ms` 的判断失效并被绕过，退化成该 Agent 的全表扫描。
const fn range_within_limit(start: i64, end: i64, max_range_ms: i64) -> bool {
    match end.checked_sub(start) {
        Some(range) => range >= 0 && range <= max_range_ms,
        None => false,
    }
}

/// 查询时间段内的所有总流量快照。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 开始时间到结束时间（含两端）之间的快照，按网卡名、快照时间排序
///
/// 1. 时间范围不合法或超过 `MAX_DETAIL_RANGE_MS` 时返回错误，未填的一端按最早快照时间、
///    当前时间计算；跨度用 `range_within_limit` 做溢出安全比较
/// 2. 查询范围内的快照，按网卡名、快照时间排序，最多返回 `DEFAULT_MONITORING_QUERY_LIMIT` 行，
///    与 `dynamic` / `static` / `dynamic_summary` 查询一致（超出部分截断，不报错）
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
    if !range_within_limit(range_start, range_end, MAX_DETAIL_RANGE_MS) {
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
        .limit(DEFAULT_MONITORING_QUERY_LIMIT)
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
    async fn range_lists_first_and_last_snapshot_per_interface() {
        let db = seeded_db().await;
        let interfaces = query_range(&db, 1).await.unwrap();
        let listed: Vec<_> = interfaces
            .iter()
            .map(|item| {
                (
                    item.interface_name.as_str(),
                    item.first_snapshot_time,
                    item.last_snapshot_time,
                )
            })
            .collect();
        assert_eq!(listed, [("eth0", at(0), at(30)), ("eth1", at(15), at(15))]);
    }

    #[tokio::test]
    async fn range_without_snapshots_is_empty() {
        let db = seeded_db().await;
        let interfaces = query_range(&db, 3).await.unwrap();
        assert!(interfaces.is_empty());
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
    async fn detail_is_clamped_to_default_monitoring_query_limit() {
        let db = seeded_db().await;
        // 额外插入超过上限的快照，确认 detail 与兄弟查询一样截断，而不是把整段范围全部返回
        let base = at(100);
        let total_rows = DEFAULT_MONITORING_QUERY_LIMIT + 5;
        let rows: Vec<_> = (0..total_rows)
            .map(|i| traffic_snapshot::ActiveModel {
                id: ActiveValue::default(),
                uuid_id: Set(1),
                interface_name: Set("eth2".to_owned()),
                snapshot_time: Set(base + i.cast_signed()),
                total_received: Set(0),
                total_transmitted: Set(0),
            })
            .collect();
        // SQLite 单条 SQL 最多绑定 999 个参数，按行分批插入
        for chunk in rows.chunks(100) {
            traffic_snapshot::Entity::insert_many(chunk.to_vec())
                .exec(&db)
                .await
                .unwrap();
        }

        let snapshots = query_detail(&db, 1, Some(base), Some(base + total_rows.cast_signed()))
            .await
            .unwrap();
        assert_eq!(snapshots.len(), DEFAULT_MONITORING_QUERY_LIMIT as usize);
    }

    /// 断言查询返回 `NodegetError::InvalidInput`。
    ///
    /// `query_detail` 的 `Ok` 类型 `Vec<TrafficSnapshotItem>` 没有实现 `Debug`，
    /// 不能用 `unwrap_err`，只能先取 `Err`。
    fn assert_invalid_input(result: anyhow::Result<Vec<TrafficSnapshotItem>>) {
        let error = result.err().expect("expected an error");
        assert!(matches!(
            error.downcast_ref::<NodegetError>(),
            Some(NodegetError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn detail_rejects_range_longer_than_limit() {
        let db = seeded_db().await;
        assert_invalid_input(
            query_detail(&db, 1, Some(at(0)), Some(at(0) + MAX_DETAIL_RANGE_MS + 1)).await,
        );
    }

    #[tokio::test]
    async fn detail_rejects_min_start_and_max_end_without_overflow() {
        let db = seeded_db().await;
        // i64::MIN 到 i64::MAX 的跨度减法会溢出：不能 panic，也不能回绕成负数绕过校验
        assert_invalid_input(query_detail(&db, 1, Some(i64::MIN), Some(i64::MAX)).await);
    }

    #[tokio::test]
    async fn detail_rejects_min_start_with_default_end() {
        let db = seeded_db().await;
        // 未填结束时间时按当前时间计算，i64::MIN 到现在的跨度同样会溢出
        assert_invalid_input(query_detail(&db, 1, Some(i64::MIN), None).await);
    }

    #[tokio::test]
    async fn detail_rejects_max_end_with_default_start() {
        let db = seeded_db().await;
        // 未填开始时间时按最早快照时间计算，到 i64::MAX 的跨度同样会溢出
        assert_invalid_input(query_detail(&db, 1, None, Some(i64::MAX)).await);
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
