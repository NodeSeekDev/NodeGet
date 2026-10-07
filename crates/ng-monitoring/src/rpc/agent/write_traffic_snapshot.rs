//! `agent.write_traffic_snapshot` RPC 实现。
//!
//! 批量写入总流量快照。快照由 JS Worker 定时从 `query_traffic_current` 取出当前总流量后写入，
//! 服务端只负责校验和落库。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{TrafficSnapshotWrite, TrafficSnapshotWriteResponse};
use crate::rpc::agent::AgentRpcImpl;
use crate::traffic_stats::max_rows_per_statement;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use ng_token::get::check_token_limit;
use sea_orm::{
    ActiveValue, DatabaseConnection, DbErr, EntityTrait, Iterable, Set, TransactionTrait,
    TryInsertResult,
};
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, warn};

/// 一次请求最多写入的快照条数
const MAX_SNAPSHOTS_PER_REQUEST: usize = 10_000;
/// 网卡名的最大长度（字符数）
const MAX_INTERFACE_NAME_CHARS: usize = 255;
/// 快照时间允许比服务端当前时间晚多久（毫秒），容忍 Worker 与服务端的时钟误差
const MAX_FUTURE_MS: i64 = 60 * 1000;

/// 批量写入总流量快照。
///
/// - `token` — 身份认证凭据
/// - `snapshots` — 要写入的快照
/// - 返回值 — `TrafficSnapshotWriteResponse`：新写入、因已存在而忽略、因设备无效而跳过的条数
///
/// 内部步骤：
/// 1. 解析 Token；快照为空时直接返回全 0
/// 2. 验证 `DynamicMonitoring::Write` 权限，`Scope` 为请求里每台设备的 `AgentUuid`
/// 3. 校验数据（`validate_snapshots`），有一条不合法整个请求失败
/// 4. 设备不存在或已软删除的条目跳过
/// 5. 在同一个事务中写入其余条目（`insert_snapshots`），同一网卡同一时间已有快照的忽略
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - 数据不合法时返回 `NodegetError::InvalidInput`
/// - 设备缓存未初始化时返回 `NodegetError::ConfigNotFound`
/// - 数据库写入失败时返回 `NodegetError::DatabaseError`
pub async fn write_traffic_snapshot(
    token: String,
    snapshots: Vec<TrafficSnapshotWrite>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        if snapshots.is_empty() {
            return serde_json::value::to_raw_value(&TrafficSnapshotWriteResponse {
                inserted: 0,
                ignored: 0,
                skipped: 0,
            })
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        let uuids: HashSet<_> = snapshots.iter().map(|snapshot| snapshot.uuid).collect();
        let scopes: Vec<Scope> = uuids.into_iter().map(Scope::AgentUuid).collect();
        let is_allowed = check_token_limit(
            &token_or_auth,
            &scopes,
            &[Permission::DynamicMonitoring(DynamicMonitoring::Write)],
        )
        .await?;
        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Write permission".to_owned(),
            )
            .into());
        }

        validate_snapshots(&snapshots, get_local_timestamp_ms_i64()?)?;

        let uuid_cache = MonitoringUuidCache::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
        })?;
        let mut rows = Vec::with_capacity(snapshots.len());
        let mut skipped = 0_u64;
        for snapshot in snapshots {
            let Some(uuid_id) = uuid_cache
                .get_id(&snapshot.uuid)
                .filter(|_| uuid_cache.is_active(&snapshot.uuid))
            else {
                skipped += 1;
                continue;
            };
            rows.push(traffic_snapshot::ActiveModel {
                id: ActiveValue::default(),
                uuid_id: Set(uuid_id),
                interface_name: Set(snapshot.interface_name),
                snapshot_time: Set(snapshot.snapshot_time),
                total_received: Set(snapshot.total_received),
                total_transmitted: Set(snapshot.total_transmitted),
            });
        }
        if skipped > 0 {
            warn!(target: "monitoring", skipped, "Traffic snapshots skipped: unknown or deleted agents");
        }

        let valid = rows.len() as u64;
        let db = AgentRpcImpl::get_db()?;
        let inserted = insert_snapshots(db, rows)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("traffic snapshot write: {e}")))?;

        debug!(target: "monitoring", inserted, skipped, "Traffic snapshots written");
        serde_json::value::to_raw_value(&TrafficSnapshotWriteResponse {
            inserted,
            ignored: valid - inserted,
            skipped,
        })
        .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// 校验要写入的快照，有一条不合法就返回错误。
///
/// - `snapshots`: 要写入的快照
/// - `now`: 服务端当前时间（毫秒）
///
/// 不合法的情况：
/// 1. 条数超过 `MAX_SNAPSHOTS_PER_REQUEST`
/// 2. 网卡名为空，或超过 `MAX_INTERFACE_NAME_CHARS` 个字符
/// 3. 快照时间小于 0，或比 `now` 晚超过 `MAX_FUTURE_MS`：时间写成未来，
///    Worker 会认为这块网卡最近已经存过快照，之后的快照都不再写
/// 4. 总接收量或总发送量小于 0
fn validate_snapshots(snapshots: &[TrafficSnapshotWrite], now: i64) -> Result<(), NodegetError> {
    if snapshots.len() > MAX_SNAPSHOTS_PER_REQUEST {
        return Err(NodegetError::InvalidInput(format!(
            "at most {MAX_SNAPSHOTS_PER_REQUEST} snapshots per request"
        )));
    }
    for snapshot in snapshots {
        let name_chars = snapshot.interface_name.chars().count();
        if name_chars == 0 || name_chars > MAX_INTERFACE_NAME_CHARS {
            return Err(NodegetError::InvalidInput(format!(
                "interface_name must be 1 to {MAX_INTERFACE_NAME_CHARS} characters"
            )));
        }
        if snapshot.snapshot_time < 0 || snapshot.snapshot_time > now.saturating_add(MAX_FUTURE_MS)
        {
            return Err(NodegetError::InvalidInput(format!(
                "snapshot_time {} of {} is negative or more than 1 minute in the future",
                snapshot.snapshot_time, snapshot.interface_name
            )));
        }
        if snapshot.total_received < 0 || snapshot.total_transmitted < 0 {
            return Err(NodegetError::InvalidInput(format!(
                "total_received and total_transmitted of {} must not be negative",
                snapshot.interface_name
            )));
        }
    }
    Ok(())
}

/// 在同一个事务中写入快照。
///
/// - `db`: 数据库连接
/// - `rows`: 要写入的快照
/// - 返回: 实际新写入的条数；同一 (设备, 网卡, 快照时间) 已存在的不写入，也不报错
///
/// 按单条 SQL 的参数上限（`max_rows_per_statement`）分批写入，任一批失败整个事务回滚。
async fn insert_snapshots(
    db: &DatabaseConnection,
    rows: Vec<traffic_snapshot::ActiveModel>,
) -> Result<u64, DbErr> {
    let chunk_rows = max_rows_per_statement(
        db.get_database_backend(),
        traffic_snapshot::Column::iter().count(),
    );
    let txn = db.begin().await?;
    let mut inserted = 0;
    for chunk in rows.chunks(chunk_rows) {
        let result = traffic_snapshot::Entity::insert_many(chunk.to_vec())
            .on_conflict_do_nothing_on([
                traffic_snapshot::Column::UuidId,
                traffic_snapshot::Column::InterfaceName,
                traffic_snapshot::Column::SnapshotTime,
            ])
            .exec_without_returning(&txn)
            .await?;
        if let TryInsertResult::Inserted(count) = result {
            inserted += count;
        }
    }
    txn.commit().await?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_stats::traffic_tables_on_sqlite;
    use sea_orm::PaginatorTrait;
    use uuid::Uuid;

    const NOW: i64 = 1_790_503_200_000;

    fn write(name: &str, time: i64, received: i64, transmitted: i64) -> TrafficSnapshotWrite {
        TrafficSnapshotWrite {
            uuid: Uuid::nil(),
            interface_name: name.to_owned(),
            snapshot_time: time,
            total_received: received,
            total_transmitted: transmitted,
        }
    }

    /// 校验这些快照，返回是否因数据不合法被拒绝
    fn is_rejected(snapshots: &[TrafficSnapshotWrite]) -> bool {
        matches!(
            validate_snapshots(snapshots, NOW),
            Err(NodegetError::InvalidInput(_))
        )
    }

    #[test]
    fn valid_snapshots_pass() {
        let snapshots = [
            write("eth0", NOW, 0, 0),
            // 恰好比服务端晚 1 分钟仍然允许
            write("eth1", NOW + MAX_FUTURE_MS, i64::MAX, i64::MAX),
            write(&"a".repeat(MAX_INTERFACE_NAME_CHARS), 0, 1, 1),
        ];
        assert!(validate_snapshots(&snapshots, NOW).is_ok());
        assert!(validate_snapshots(&[], NOW).is_ok());
    }

    #[test]
    fn too_many_snapshots_are_rejected() {
        let snapshots: Vec<_> = (0..=MAX_SNAPSHOTS_PER_REQUEST)
            .map(|_| write("eth0", NOW, 0, 0))
            .collect();
        assert!(is_rejected(&snapshots));
    }

    #[test]
    fn bad_interface_names_are_rejected() {
        assert!(is_rejected(&[write("", NOW, 0, 0)]));
        let too_long = "a".repeat(MAX_INTERFACE_NAME_CHARS + 1);
        assert!(is_rejected(&[write(&too_long, NOW, 0, 0)]));
    }

    #[test]
    fn bad_times_are_rejected() {
        assert!(is_rejected(&[write("eth0", -1, 0, 0)]));
        assert!(is_rejected(&[write("eth0", NOW + MAX_FUTURE_MS + 1, 0, 0)]));
    }

    #[test]
    fn negative_totals_are_rejected() {
        assert!(is_rejected(&[write("eth0", NOW, -1, 0)]));
        assert!(is_rejected(&[write("eth0", NOW, 0, -1)]));
    }

    #[test]
    fn one_bad_entry_rejects_the_whole_request() {
        assert!(is_rejected(&[
            write("eth0", NOW, 0, 0),
            write("eth1", NOW, -1, 0)
        ]));
    }

    fn row(uuid_id: i16, name: &str, time: i64, received: i64) -> traffic_snapshot::ActiveModel {
        traffic_snapshot::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            interface_name: Set(name.to_owned()),
            snapshot_time: Set(time),
            total_received: Set(received),
            total_transmitted: Set(0),
        }
    }

    #[tokio::test]
    async fn insert_counts_only_new_rows_and_keeps_existing_ones() {
        let db = traffic_tables_on_sqlite().await;
        let first = insert_snapshots(&db, vec![row(1, "eth0", 100, 5), row(1, "eth0", 200, 6)])
            .await
            .unwrap();
        assert_eq!(first, 2);

        // 100 已存在被忽略，旧值不被覆盖；300 是新的；请求内重复的 400 只写一条
        let second = insert_snapshots(
            &db,
            vec![
                row(1, "eth0", 100, 999),
                row(1, "eth0", 300, 7),
                row(1, "eth0", 400, 8),
                row(1, "eth0", 400, 9),
            ],
        )
        .await
        .unwrap();
        assert_eq!(second, 2);

        let rows = traffic_snapshot::Entity::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 4);
        let at_100 = rows.iter().find(|row| row.snapshot_time == 100).unwrap();
        assert_eq!(at_100.total_received, 5);
    }

    #[tokio::test]
    async fn insert_splits_large_batches() {
        let db = traffic_tables_on_sqlite().await;
        // 超过 SQLite 单条 SQL 能写的行数，会分批写入
        let rows: Vec<_> = (0..1000).map(|i| row(1, "eth0", i, i)).collect();
        assert_eq!(insert_snapshots(&db, rows).await.unwrap(), 1000);
        let count = traffic_snapshot::Entity::find().count(&db).await.unwrap();
        assert_eq!(count, 1000);
    }

    #[tokio::test]
    async fn insert_nothing_returns_zero() {
        let db = traffic_tables_on_sqlite().await;
        assert_eq!(insert_snapshots(&db, Vec::new()).await.unwrap(), 0);
    }
}
