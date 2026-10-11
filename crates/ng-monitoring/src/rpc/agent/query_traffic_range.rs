//! `agent.query_traffic_range` RPC 实现。
//!
//! 查询一台设备每块网卡有快照数据的时间范围，数据来自 `traffic_snapshot` 表。

use crate::query::{InterfaceSnapshotRangeItem, TrafficRangeResponse};
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::query_traffic_common::{authorize_and_resolve, database_error};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;
use uuid::Uuid;

/// 查询每块网卡有快照数据的时间范围。
///
/// - `token` — 身份认证凭据
/// - `uuid` — 设备 UUID
/// - 返回值 — `TrafficRangeResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 查询每块网卡最早和最晚的快照时间（`query_range`）
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic_range(token: String, uuid: Uuid) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let uuid_id = authorize_and_resolve(&token, uuid).await?;

        let db = AgentRpcImpl::get_db()?;
        let interfaces = query_range(db, uuid_id).await?;
        let response = serde_json::value::to_raw_value(&TrafficRangeResponse { uuid, interfaces })
            .map_err(|e| NodegetError::SerializationError(format!("traffic response: {e}")))?;

        debug!(target: "monitoring", uuid = %uuid, "Traffic range query completed");
        Ok(response)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::agent::query_traffic_common::test_support::{at, seeded_db};

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
}
