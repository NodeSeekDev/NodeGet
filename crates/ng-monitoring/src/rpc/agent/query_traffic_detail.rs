//! `agent.query_traffic_detail` RPC 实现。
//!
//! 查询一台设备在指定时间段内的每一条总流量快照。
//! 数据来自 `traffic_snapshot`、`traffic_possible_data_loss` 两张表，不调用 `TrafficStats`。

use crate::query::{TrafficDetailQuery, TrafficDetailResponse, TrafficSnapshotItem};
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::query_traffic_common::{
    authorize_and_resolve, check_time_order, database_error, query_possible_data_losses,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::value::RawValue;
use tracing::debug;

/// `detail` 的最长时间范围（毫秒），92 天
const MAX_DETAIL_RANGE_MS: i64 = 92 * 24 * 60 * 60 * 1000;

/// 查询一段时间内的所有总流量快照。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `TrafficDetailResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间时返回错误
/// 4. 查询时间段内的快照（`query_detail`），以及可能丢失数据的时间段（`query_possible_data_losses`）
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 开始时间晚于结束时间、时间范围超过 `MAX_DETAIL_RANGE_MS` 时返回 `NodegetError::InvalidInput`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic_detail(
    token: String,
    query: TrafficDetailQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let uuid_id = authorize_and_resolve(&token, query.uuid).await?;
        check_time_order(query.start_time, query.end_time)?;

        let db = AgentRpcImpl::get_db()?;
        let possible_data_losses =
            query_possible_data_losses(db, uuid_id, query.start_time, query.end_time).await?;
        let snapshots = query_detail(db, uuid_id, query.start_time, query.end_time).await?;
        let response = serde_json::value::to_raw_value(&TrafficDetailResponse {
            uuid: query.uuid,
            start_time: query.start_time,
            end_time: query.end_time,
            snapshots,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::agent::query_traffic_common::test_support::{at, seeded_db};

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
}
