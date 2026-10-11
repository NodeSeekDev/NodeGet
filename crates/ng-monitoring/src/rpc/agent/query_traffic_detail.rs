//! `agent.query_traffic_detail` RPC 实现。
//!
//! 查询一台设备在指定时间段内的每一条总流量快照，分页返回。
//! 数据来自 `traffic_snapshot`、`traffic_possible_data_loss` 两张表，不调用 `TrafficStats`。

use crate::query::{TrafficCursor, TrafficDetailQuery, TrafficDetailResponse, TrafficSnapshotItem};
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::query_traffic_common::{
    authorize_and_resolve, check_time_order, database_error, query_possible_data_losses,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::utils::{DEFAULT_MONITORING_QUERY_LIMIT, MAX_QUERY_LIMIT};
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::value::RawValue;
use tracing::debug;

/// 查询一页总流量快照。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `TrafficDetailResponse`，后面还有数据时带 `next_cursor`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间、每页条数不合法时返回错误
/// 4. 查询这一页快照（`query_detail`），以及可能丢失数据的时间段（`query_possible_data_losses`），
///    每一页都返回后者
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 开始时间晚于结束时间、每页条数为 0 时返回 `NodegetError::InvalidInput`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic_detail(
    token: String,
    query: TrafficDetailQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let uuid_id = authorize_and_resolve(&token, query.uuid).await?;
        check_time_order(query.start_time, query.end_time)?;
        let page_size = resolve_page_size(query.limit)?;

        let db = AgentRpcImpl::get_db()?;
        let possible_data_losses =
            query_possible_data_losses(db, uuid_id, query.start_time, query.end_time).await?;
        let (snapshots, next_cursor) = query_detail(
            db,
            uuid_id,
            query.start_time,
            query.end_time,
            page_size,
            query.cursor.as_ref(),
        )
        .await?;
        let response = serde_json::value::to_raw_value(&TrafficDetailResponse {
            uuid: query.uuid,
            start_time: query.start_time,
            end_time: query.end_time,
            snapshots,
            next_cursor,
            possible_data_losses,
        })
        .map_err(|e| NodegetError::SerializationError(format!("traffic response: {e}")))?;

        debug!(target: "monitoring", uuid = %query.uuid, "Traffic detail query completed");
        Ok(response)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// 确定每页返回多少条。
///
/// - `limit`: 客户端指定的每页条数
/// - 返回: 未指定时为 `DEFAULT_MONITORING_QUERY_LIMIT`，超过 `MAX_QUERY_LIMIT` 按 `MAX_QUERY_LIMIT` 处理
///
/// # Errors
///
/// - 每页条数为 0 时返回 `NodegetError::InvalidInput`：返回空页分不清是没有数据还是参数错了
fn resolve_page_size(limit: Option<u64>) -> anyhow::Result<u64> {
    match limit {
        None => Ok(DEFAULT_MONITORING_QUERY_LIMIT),
        Some(0) => {
            Err(NodegetError::InvalidInput("limit must be greater than 0".to_owned()).into())
        }
        Some(limit) => Ok(limit.min(MAX_QUERY_LIMIT)),
    }
}

/// 查询一页总流量快照。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - `page_size`: 每页最多返回的条数
/// - `cursor`: 翻页位置，`None` 表示第一页
/// - 返回: 这一页的快照（开始时间到结束时间之间，含两端，按网卡名、快照时间排序），
///   后面还有数据时附带下一页的翻页位置
///
/// 翻页位置是已经返回的最后一条快照的（网卡名, 快照时间），下一页取排在它后面的快照，不包含它。
/// 多查一条来判断后面还有没有数据。翻页期间有新快照写入，不会造成重复或遗漏。
async fn query_detail(
    db: &DatabaseConnection,
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
    page_size: u64,
    cursor: Option<&TrafficCursor>,
) -> anyhow::Result<(Vec<TrafficSnapshotItem>, Option<TrafficCursor>)> {
    let mut select =
        traffic_snapshot::Entity::find().filter(traffic_snapshot::Column::UuidId.eq(uuid_id));
    if let Some(start_time) = start_time {
        select = select.filter(traffic_snapshot::Column::SnapshotTime.gte(start_time));
    }
    if let Some(end_time) = end_time {
        select = select.filter(traffic_snapshot::Column::SnapshotTime.lte(end_time));
    }
    if let Some(cursor) = cursor {
        // 排在翻页位置后面：网卡名更大，或者同一块网卡且快照时间更晚
        select = select.filter(
            Condition::any()
                .add(traffic_snapshot::Column::InterfaceName.gt(cursor.interface_name.as_str()))
                .add(
                    Condition::all()
                        .add(
                            traffic_snapshot::Column::InterfaceName
                                .eq(cursor.interface_name.as_str()),
                        )
                        .add(traffic_snapshot::Column::SnapshotTime.gt(cursor.snapshot_time)),
                ),
        );
    }

    let mut snapshots = select
        .order_by_asc(traffic_snapshot::Column::InterfaceName)
        .order_by_asc(traffic_snapshot::Column::SnapshotTime)
        .limit(page_size.saturating_add(1))
        .all(db)
        .await
        .map_err(|e| database_error(&e))?;

    // 多查出来的那一条说明后面还有数据，不返回
    let has_more = snapshots.len() as u64 > page_size;
    snapshots.truncate(page_size as usize);
    let next_cursor = if has_more {
        snapshots.last().map(|last| TrafficCursor {
            interface_name: last.interface_name.clone(),
            snapshot_time: last.snapshot_time,
        })
    } else {
        None
    };

    let items = snapshots
        .into_iter()
        .map(|snapshot| TrafficSnapshotItem {
            interface_name: snapshot.interface_name,
            snapshot_time: snapshot.snapshot_time,
            total_received: snapshot.total_received.cast_unsigned(),
            total_transmitted: snapshot.total_transmitted.cast_unsigned(),
        })
        .collect();
    Ok((items, next_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::agent::query_traffic_common::test_support::{at, seeded_db};

    /// 把快照变成 (网卡名, 快照时间) 列表，方便比较
    fn listed(snapshots: &[TrafficSnapshotItem]) -> Vec<(&str, i64)> {
        snapshots
            .iter()
            .map(|s| (s.interface_name.as_str(), s.snapshot_time))
            .collect()
    }

    /// 翻页位置
    fn cursor(interface_name: &str, snapshot_time: i64) -> TrafficCursor {
        TrafficCursor {
            interface_name: interface_name.to_owned(),
            snapshot_time,
        }
    }

    #[tokio::test]
    async fn detail_returns_snapshots_in_range_ordered() {
        let db = seeded_db().await;
        let (snapshots, next_cursor) =
            query_detail(&db, 1, Some(at(10)), Some(at(30)), 10_000, None)
                .await
                .unwrap();
        assert_eq!(
            listed(&snapshots),
            [("eth0", at(15)), ("eth0", at(30)), ("eth1", at(15))]
        );
        assert_eq!(snapshots[0].total_received, 150);
        assert!(next_cursor.is_none());
    }

    #[tokio::test]
    async fn detail_pages_cover_every_snapshot_once() {
        let db = seeded_db().await;
        // 每页 3 条：eth0 的三条在第一页，eth1 的一条在第二页
        let (first, next_cursor) = query_detail(&db, 1, None, None, 3, None).await.unwrap();
        assert_eq!(
            listed(&first),
            [("eth0", at(0)), ("eth0", at(15)), ("eth0", at(30))]
        );
        assert_eq!(next_cursor, Some(cursor("eth0", at(30))));

        let (second, next_cursor) = query_detail(&db, 1, None, None, 3, next_cursor.as_ref())
            .await
            .unwrap();
        assert_eq!(listed(&second), [("eth1", at(15))]);
        assert!(next_cursor.is_none());
    }

    #[tokio::test]
    async fn detail_page_ending_exactly_at_the_last_snapshot_has_no_next_cursor() {
        let db = seeded_db().await;
        // 一共 4 条，每页正好 4 条：后面没有数据
        let (snapshots, next_cursor) = query_detail(&db, 1, None, None, 4, None).await.unwrap();
        assert_eq!(snapshots.len(), 4);
        assert!(next_cursor.is_none());
    }

    #[tokio::test]
    async fn detail_cursor_inside_an_interface_continues_after_it() {
        let db = seeded_db().await;
        // 每页 2 条，从 eth0 的 10:15 之后继续
        let (snapshots, next_cursor) =
            query_detail(&db, 1, None, None, 2, Some(&cursor("eth0", at(15))))
                .await
                .unwrap();
        assert_eq!(listed(&snapshots), [("eth0", at(30)), ("eth1", at(15))]);
        assert!(next_cursor.is_none());
    }

    #[tokio::test]
    async fn detail_has_no_time_range_limit_and_no_overflow() {
        let db = seeded_db().await;
        // 以前开始时间取 i64::MIN 会让时间差溢出，现在不做减法，范围再大也只受每页条数限制
        let (snapshots, _) = query_detail(&db, 1, Some(i64::MIN), Some(i64::MAX), 10_000, None)
            .await
            .unwrap();
        assert_eq!(snapshots.len(), 4);
    }

    #[tokio::test]
    async fn detail_end_before_the_first_snapshot_is_empty() {
        let db = seeded_db().await;
        let (snapshots, next_cursor) = query_detail(&db, 1, None, Some(at(-60)), 10_000, None)
            .await
            .unwrap();
        assert!(snapshots.is_empty());
        assert!(next_cursor.is_none());
    }

    #[test]
    fn page_size_defaults_clamps_and_rejects_zero() {
        assert_eq!(
            resolve_page_size(None).unwrap(),
            DEFAULT_MONITORING_QUERY_LIMIT
        );
        assert_eq!(resolve_page_size(Some(5)).unwrap(), 5);
        assert_eq!(
            resolve_page_size(Some(MAX_QUERY_LIMIT + 1)).unwrap(),
            MAX_QUERY_LIMIT
        );
        let error = resolve_page_size(Some(0)).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<NodegetError>(),
            Some(NodegetError::InvalidInput(_))
        ));
    }
}
