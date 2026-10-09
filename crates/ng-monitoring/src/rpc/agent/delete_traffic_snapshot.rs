//! `agent.delete_traffic_snapshot` RPC 实现。
//!
//! 删除指定时间及之前的总流量快照，以及整段早于该时间的可能丢失数据的时间段。
//! 供 JS Worker 定时清理过期数据使用。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::TrafficSnapshotDeleteResponse;
use crate::rpc::agent::AgentRpcImpl;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::{traffic_possible_data_loss, traffic_snapshot};
use ng_infra::server::{RpcHelper, to_rpc_error};
use ng_token::get::check_token_limit;
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::debug;
use uuid::Uuid;

/// 每条 `DELETE` 最多删除的快照条数。
///
/// 分批删除，避免第一次清理大量历史数据时，在 `SQLite` 上长时间占着写锁，
/// 让其他写入等到超时
const DELETE_BATCH_SIZE: u64 = 5000;

/// 删除总流量快照。
///
/// - `token` — 身份认证凭据
/// - `end_time` — 删除快照时间小于等于它的快照（毫秒）
/// - `uuid` — 只删这台设备的快照；不填表示所有设备（包括已软删除的）
/// - 返回值 — `TrafficSnapshotDeleteResponse`：删除的快照条数和可能丢失数据的时间段条数
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Delete` 权限：指定设备时 `Scope` 为 `AgentUuid`，
///    不指定时为 `Global`
/// 2. 指定了设备时通过 `MonitoringUuidCache` 转为 `uuid_id`，软删除的设备也可以清理
/// 3. 分批删除快照（`delete_snapshots`），每批一条独立的 SQL，不放在同一个事务里
/// 4. 删除结束时间小于等于 `end_time` 的可能丢失数据的时间段
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - 指定的设备不存在时返回 `NodegetError::NotFound`
/// - 设备缓存未初始化时返回 `NodegetError::ConfigNotFound`
/// - 数据库删除失败时返回 `NodegetError::DatabaseError`
pub async fn delete_traffic_snapshot(
    token: String,
    end_time: i64,
    uuid: Option<Uuid>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let scope = uuid.map_or(Scope::Global, Scope::AgentUuid);
        let is_allowed = check_token_limit(
            &token_or_auth,
            &[scope],
            &[Permission::DynamicMonitoring(DynamicMonitoring::Delete)],
        )
        .await?;
        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Delete permission".to_owned(),
            )
            .into());
        }

        let uuid_id = match uuid {
            Some(uuid) => Some(
                MonitoringUuidCache::global()
                    .ok_or_else(|| {
                        NodegetError::ConfigNotFound(
                            "MonitoringUuidCache not initialized".to_owned(),
                        )
                    })?
                    .get_id(&uuid)
                    .ok_or_else(|| NodegetError::NotFound(format!("Unknown agent UUID: {uuid}")))?,
            ),
            None => None,
        };

        let db = AgentRpcImpl::get_db()?;
        let deleted_snapshots = delete_snapshots(db, end_time, uuid_id, DELETE_BATCH_SIZE)
            .await
            .map_err(|e| database_error(&e))?;
        let deleted_possible_data_losses = delete_possible_data_losses(db, end_time, uuid_id)
            .await
            .map_err(|e| database_error(&e))?;

        debug!(target: "monitoring", deleted_snapshots, deleted_possible_data_losses, "Traffic snapshots deleted");
        serde_json::value::to_raw_value(&TrafficSnapshotDeleteResponse {
            deleted_snapshots,
            deleted_possible_data_losses,
        })
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// 分批删除快照时间小于等于 `end_time` 的快照。
///
/// - `db`: 数据库连接
/// - `end_time`: 快照时间上限（毫秒，含）
/// - `uuid_id`: 设备编号，`None` 表示所有设备
/// - `batch_size`: 每条 `DELETE` 最多删除的条数
/// - 返回: 删除的总条数
///
/// 每批先在子查询里选出最多 `batch_size` 条的编号再删除，一批删不满说明已删完。
async fn delete_snapshots(
    db: &DatabaseConnection,
    end_time: i64,
    uuid_id: Option<i16>,
    batch_size: u64,
) -> Result<u64, DbErr> {
    let mut deleted_total = 0;
    loop {
        let mut batch = Query::select();
        batch
            .column(traffic_snapshot::Column::Id)
            .from(traffic_snapshot::Entity)
            .and_where(traffic_snapshot::Column::SnapshotTime.lte(end_time))
            .limit(batch_size);
        if let Some(uuid_id) = uuid_id {
            batch.and_where(traffic_snapshot::Column::UuidId.eq(uuid_id));
        }

        let deleted = traffic_snapshot::Entity::delete_many()
            .filter(traffic_snapshot::Column::Id.in_subquery(batch))
            .exec(db)
            .await?
            .rows_affected;
        deleted_total += deleted;
        if deleted < batch_size {
            return Ok(deleted_total);
        }
    }
}

/// 删除结束时间小于等于 `end_time` 的可能丢失数据的时间段。
///
/// - `db`: 数据库连接
/// - `end_time`: 时间上限（毫秒，含）
/// - `uuid_id`: 设备编号，`None` 表示所有设备
/// - 返回: 删除的条数
///
/// 时间段只在整段都早于 `end_time` 时才删，跨过 `end_time` 的保留。
async fn delete_possible_data_losses(
    db: &DatabaseConnection,
    end_time: i64,
    uuid_id: Option<i16>,
) -> Result<u64, DbErr> {
    let mut delete = traffic_possible_data_loss::Entity::delete_many()
        .filter(traffic_possible_data_loss::Column::EndTime.lte(end_time));
    if let Some(uuid_id) = uuid_id {
        delete = delete.filter(traffic_possible_data_loss::Column::UuidId.eq(uuid_id));
    }
    Ok(delete.exec(db).await?.rows_affected)
}

/// 把数据库错误转成 `NodegetError::DatabaseError`。
fn database_error(e: &DbErr) -> NodegetError {
    NodegetError::DatabaseError(format!("traffic snapshot delete: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_stats::traffic_tables_on_sqlite;
    use sea_orm::{ActiveValue, PaginatorTrait, Set};

    /// 设备 1 的 eth0 在时间 1..=5 各一条快照，设备 2 的 eth0 在时间 1..=3 各一条
    async fn seeded_db() -> DatabaseConnection {
        let db = traffic_tables_on_sqlite().await;
        let snapshot = |uuid_id: i16, time: i64| traffic_snapshot::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            interface_name: Set("eth0".to_owned()),
            snapshot_time: Set(time),
            total_received: Set(0),
            total_transmitted: Set(0),
        };
        let rows: Vec<_> = (1..=5)
            .map(|time| snapshot(1, time))
            .chain((1..=3).map(|time| snapshot(2, time)))
            .collect();
        traffic_snapshot::Entity::insert_many(rows)
            .exec(&db)
            .await
            .unwrap();

        let loss = |uuid_id: i16, start: i64, end: i64| traffic_possible_data_loss::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            start_time: Set(start),
            end_time: Set(end),
        };
        traffic_possible_data_loss::Entity::insert_many([
            loss(1, 0, 2),
            loss(1, 3, 6),
            loss(2, 0, 1),
        ])
        .exec(&db)
        .await
        .unwrap();
        db
    }

    async fn snapshot_count(db: &DatabaseConnection) -> u64 {
        traffic_snapshot::Entity::find().count(db).await.unwrap()
    }

    #[tokio::test]
    async fn deletes_in_batches_until_everything_up_to_end_time_is_gone() {
        let db = seeded_db().await;
        // 每批 2 条，要循环多次：时间 <= 3 的共 6 条（设备 1 三条、设备 2 三条）
        let deleted = delete_snapshots(&db, 3, None, 2).await.unwrap();
        assert_eq!(deleted, 6);
        assert_eq!(snapshot_count(&db).await, 2);
    }

    #[tokio::test]
    async fn batch_boundary_is_handled_when_count_is_a_multiple_of_batch_size() {
        let db = seeded_db().await;
        // 时间 <= 2 的共 4 条，每批 2 条：第二批刚好删满，要再查一批才知道结束
        let deleted = delete_snapshots(&db, 2, None, 2).await.unwrap();
        assert_eq!(deleted, 4);
        assert_eq!(snapshot_count(&db).await, 4);
    }

    #[tokio::test]
    async fn only_given_device_is_deleted() {
        let db = seeded_db().await;
        let deleted = delete_snapshots(&db, 10, Some(1), 100).await.unwrap();
        assert_eq!(deleted, 5);
        assert_eq!(snapshot_count(&db).await, 3);
    }

    #[tokio::test]
    async fn nothing_to_delete_returns_zero() {
        let db = seeded_db().await;
        assert_eq!(delete_snapshots(&db, 0, None, 2).await.unwrap(), 0);
        assert_eq!(snapshot_count(&db).await, 8);
    }

    #[tokio::test]
    async fn possible_data_losses_are_deleted_only_when_wholly_before_end_time() {
        let db = seeded_db().await;
        // 时间段 (3,6) 跨过 4，保留；(0,2) 和 (0,1) 删除
        let deleted = delete_possible_data_losses(&db, 4, None).await.unwrap();
        assert_eq!(deleted, 2);
        let left = traffic_possible_data_loss::Entity::find()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].end_time, 6);
    }

    #[tokio::test]
    async fn possible_data_losses_of_given_device_only() {
        let db = seeded_db().await;
        let deleted = delete_possible_data_losses(&db, 10, Some(2)).await.unwrap();
        assert_eq!(deleted, 1);
        let left = traffic_possible_data_loss::Entity::find()
            .count(&db)
            .await
            .unwrap();
        assert_eq!(left, 2);
    }
}
