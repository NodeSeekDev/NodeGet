//! `agent.query_traffic_current` RPC 实现。
//!
//! 查询设备当前的总流量。总流量来自 `TrafficStats` 的内存状态，
//! 另从 `traffic_snapshot` 表取每块网卡最晚的快照时间，供 Worker 判断是否该写新快照。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{DeviceCurrentTraffic, DynamicDataQueryField, InterfaceCurrentTrafficItem};
use crate::rpc::agent::AgentRpcImpl;
use crate::traffic_stats::{InterfaceCurrentTotal, TrafficStats};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::traffic_snapshot;
use ng_infra::server::{RpcHelper, to_rpc_error};
use ng_token::get::check_token_limit;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use tracing::debug;
use uuid::Uuid;

/// 一次查询最后快照时间时，`IN` 条件里最多放多少个设备编号。
///
/// 小于 `SQLite` 单条 SQL 的参数上限（999）
const UUID_ID_CHUNK: usize = 500;

/// 查询设备当前的总流量。
///
/// - `token` — 身份认证凭据
/// - `uuids` — 设备 UUID 列表（自动去重）；不填表示所有设备
/// - 返回值 — `DeviceCurrentTraffic` 数组，按设备 UUID 排序
///
/// 内部步骤：
/// 1. 解析 Token；指定了设备时列表为空直接返回 `[]`
/// 2. 验证 `DynamicMonitoring::Read(Network)` 权限：指定设备时 `Scope` 为各设备的 `AgentUuid`，
///    不指定设备时为 `Global`
/// 3. 从 `TrafficStats` 取所有网卡的总流量，只保留指定的、未软删除的设备
///    （`select_totals`）
/// 4. 查这些设备每块网卡最晚的快照时间（`last_snapshot_times`）
/// 5. 按设备、网卡名排序组装结果（`assemble`）
///
/// 不存在的设备、已软删除的设备、没有出口网卡的设备都不会出现在结果里。
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - 流量统计或设备缓存未初始化时返回 `NodegetError::ConfigNotFound`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic_current(
    token: String,
    uuids: Option<Vec<Uuid>>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let requested = uuids.map(dedupe_uuids);
        if requested.as_ref().is_some_and(Vec::is_empty) {
            return RawValue::from_string("[]".to_owned())
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        let scopes: Vec<Scope> = requested.as_ref().map_or_else(
            || vec![Scope::Global],
            |uuids| uuids.iter().map(|uuid| Scope::AgentUuid(*uuid)).collect(),
        );
        let is_allowed = check_token_limit(
            &token_or_auth,
            &scopes,
            &[Permission::DynamicMonitoring(DynamicMonitoring::Read(
                DynamicDataQueryField::Network,
            ))],
        )
        .await?;
        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Read(network) permission".to_owned(),
            )
            .into());
        }

        let uuid_cache = MonitoringUuidCache::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
        })?;
        let wanted: Option<HashSet<i16>> = requested.map(|uuids| {
            uuids
                .iter()
                .filter_map(|uuid| uuid_cache.get_id(uuid))
                .collect()
        });
        let totals = TrafficStats::current_totals().ok_or_else(|| {
            NodegetError::ConfigNotFound("TrafficStats not initialized".to_owned())
        })?;
        let selected = select_totals(totals, wanted.as_ref(), |uuid_id| {
            uuid_cache
                .get_uuid(uuid_id)
                .filter(|uuid| uuid_cache.is_active(uuid))
        });

        let db = AgentRpcImpl::get_db()?;
        let uuid_ids: Vec<i16> = selected
            .iter()
            .map(|(_, total)| total.uuid_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let last_snapshots = last_snapshot_times(db, &uuid_ids).await?;
        let devices = assemble(selected, &last_snapshots);

        debug!(target: "monitoring", devices = devices.len(), "Current traffic query completed");
        serde_json::value::to_raw_value(&devices)
            .map_err(|e| NodegetError::SerializationError(format!("traffic current: {e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}

/// UUID 列表去重，保持原始顺序。
fn dedupe_uuids(uuids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = HashSet::with_capacity(uuids.len());
    uuids
        .into_iter()
        .filter(|uuid| seen.insert(*uuid))
        .collect()
}

/// 挑出要返回的网卡，并换成设备 UUID。
///
/// - `totals`: 所有网卡当前的总流量
/// - `wanted`: 指定的设备编号，`None` 表示所有设备
/// - `resolve`: 把设备编号换成设备 UUID，设备不存在或已软删除时返回 `None`
/// - 返回: 指定的、`resolve` 能解析的设备的网卡
fn select_totals(
    totals: Vec<InterfaceCurrentTotal>,
    wanted: Option<&HashSet<i16>>,
    resolve: impl Fn(i16) -> Option<Uuid>,
) -> Vec<(Uuid, InterfaceCurrentTotal)> {
    totals
        .into_iter()
        .filter(|total| wanted.is_none_or(|wanted| wanted.contains(&total.uuid_id)))
        .filter_map(|total| resolve(total.uuid_id).map(|uuid| (uuid, total)))
        .collect()
}

/// 查询每块网卡最晚的快照时间。
///
/// - `db`: 数据库连接
/// - `uuid_ids`: 设备编号
/// - 返回: (设备编号, 网卡名) → 最晚的快照时间；没有快照的网卡不在其中
async fn last_snapshot_times(
    db: &DatabaseConnection,
    uuid_ids: &[i16],
) -> anyhow::Result<HashMap<(i16, String), i64>> {
    let mut last_times = HashMap::new();
    for chunk in uuid_ids.chunks(UUID_ID_CHUNK) {
        let rows: Vec<(i16, String, Option<i64>)> = traffic_snapshot::Entity::find()
            .select_only()
            .column(traffic_snapshot::Column::UuidId)
            .column(traffic_snapshot::Column::InterfaceName)
            .column_as(
                traffic_snapshot::Column::SnapshotTime.max(),
                "last_snapshot_time",
            )
            .filter(traffic_snapshot::Column::UuidId.is_in(chunk.to_vec()))
            .group_by(traffic_snapshot::Column::UuidId)
            .group_by(traffic_snapshot::Column::InterfaceName)
            .into_tuple()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("traffic current query: {e}")))?;
        for (uuid_id, interface_name, last_time) in rows {
            if let Some(last_time) = last_time {
                last_times.insert((uuid_id, interface_name), last_time);
            }
        }
    }
    Ok(last_times)
}

/// 按设备、网卡名排序，组装返回结果。
///
/// - `selected`: `select_totals` 的结果
/// - `last_snapshots`: `last_snapshot_times` 的结果
fn assemble(
    selected: Vec<(Uuid, InterfaceCurrentTotal)>,
    last_snapshots: &HashMap<(i16, String), i64>,
) -> Vec<DeviceCurrentTraffic> {
    let mut devices: BTreeMap<Uuid, Vec<InterfaceCurrentTrafficItem>> = BTreeMap::new();
    for (uuid, total) in selected {
        let last_snapshot_time = last_snapshots
            .get(&(total.uuid_id, total.interface_name.clone()))
            .copied();
        devices
            .entry(uuid)
            .or_default()
            .push(InterfaceCurrentTrafficItem {
                interface_name: total.interface_name,
                total_received: total.total_received,
                total_transmitted: total.total_transmitted,
                updated_at: total.updated_at,
                last_snapshot_time,
            });
    }
    devices
        .into_iter()
        .map(|(uuid, mut interfaces)| {
            interfaces.sort_by(|a, b| a.interface_name.cmp(&b.interface_name));
            DeviceCurrentTraffic { uuid, interfaces }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_stats::traffic_tables_on_sqlite;
    use sea_orm::{ActiveValue, Set};

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn total(uuid_id: i16, name: &str, received: u64) -> InterfaceCurrentTotal {
        InterfaceCurrentTotal {
            uuid_id,
            interface_name: name.to_owned(),
            total_received: received,
            total_transmitted: received / 10,
            updated_at: 1000,
        }
    }

    /// 设备编号 n 对应 UUID n，编号 9 视为已软删除
    fn resolve(uuid_id: i16) -> Option<Uuid> {
        (uuid_id != 9).then(|| uuid(u128::from(uuid_id.cast_unsigned())))
    }

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let deduped = dedupe_uuids(vec![uuid(2), uuid(1), uuid(2), uuid(3), uuid(1)]);
        assert_eq!(deduped, [uuid(2), uuid(1), uuid(3)]);
    }

    #[test]
    fn select_all_devices_drops_unresolvable_ones() {
        let totals = vec![
            total(1, "eth0", 10),
            total(9, "eth0", 99),
            total(2, "eth0", 20),
        ];
        let selected = select_totals(totals, None, resolve);
        let uuids: Vec<_> = selected.iter().map(|(uuid, _)| *uuid).collect();
        assert_eq!(uuids, [uuid(1), uuid(2)]);
    }

    #[test]
    fn select_specific_devices_ignores_others_and_deleted() {
        let totals = vec![
            total(1, "eth0", 10),
            total(9, "eth0", 99),
            total(2, "eth0", 20),
        ];
        let wanted = HashSet::from([2, 9]);
        let selected = select_totals(totals, Some(&wanted), resolve);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].0, uuid(2));
    }

    #[test]
    fn assemble_sorts_by_uuid_then_interface_and_fills_last_snapshot_time() {
        let selected = vec![
            (uuid(2), total(2, "eth0", 20)),
            (uuid(1), total(1, "eth1", 11)),
            (uuid(1), total(1, "eth0", 10)),
        ];
        let last = HashMap::from([((1, "eth0".to_owned()), 500)]);
        let devices = assemble(selected, &last);

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].uuid, uuid(1));
        let names: Vec<_> = devices[0]
            .interfaces
            .iter()
            .map(|item| (item.interface_name.as_str(), item.last_snapshot_time))
            .collect();
        assert_eq!(names, [("eth0", Some(500)), ("eth1", None)]);
        assert_eq!(devices[1].uuid, uuid(2));
        assert_eq!(devices[1].interfaces[0].total_received, 20);
        assert_eq!(devices[1].interfaces[0].total_transmitted, 2);
    }

    #[tokio::test]
    async fn last_snapshot_times_returns_latest_per_interface_and_only_requested_devices() {
        let db = traffic_tables_on_sqlite().await;
        let snapshot = |uuid_id: i16, name: &str, time: i64| traffic_snapshot::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            interface_name: Set(name.to_owned()),
            snapshot_time: Set(time),
            total_received: Set(0),
            total_transmitted: Set(0),
        };
        traffic_snapshot::Entity::insert_many([
            snapshot(1, "eth0", 100),
            snapshot(1, "eth0", 300),
            snapshot(1, "eth0", 200),
            snapshot(1, "eth1", 50),
            snapshot(2, "eth0", 700),
        ])
        .exec(&db)
        .await
        .unwrap();

        let last = last_snapshot_times(&db, &[1]).await.unwrap();
        assert_eq!(last.len(), 2);
        assert_eq!(last[&(1, "eth0".to_owned())], 300);
        assert_eq!(last[&(1, "eth1".to_owned())], 50);
        assert!(last_snapshot_times(&db, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn last_snapshot_times_handles_more_devices_than_one_chunk() {
        let db = traffic_tables_on_sqlite().await;
        traffic_snapshot::Entity::insert(traffic_snapshot::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(1200),
            interface_name: Set("eth0".to_owned()),
            snapshot_time: Set(42),
            total_received: Set(0),
            total_transmitted: Set(0),
        })
        .exec(&db)
        .await
        .unwrap();

        // 超过 SQLite 单条 SQL 的参数上限，必须分批查
        let uuid_ids: Vec<i16> = (1..=2000).collect();
        let last = last_snapshot_times(&db, &uuid_ids).await.unwrap();
        assert_eq!(last[&(1200, "eth0".to_owned())], 42);
    }
}
