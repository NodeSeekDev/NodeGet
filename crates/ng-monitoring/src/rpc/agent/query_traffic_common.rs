//! 流量查询接口的公共函数。
//!
//! 权限检查与 UUID 转换、开始/结束时间校验、可能丢失数据的时间段查询，
//! 供 `query_traffic_total`、`query_traffic_detail`、`query_traffic_range` 使用。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{DynamicDataQueryField, PossibleDataLossItem};
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::traffic_possible_data_loss;
use ng_token::get::check_token_limit;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

/// 验证 Token 对指定设备有查询网络数据的权限，并把设备 UUID 转为 `uuid_id`。
///
/// - `token`: 身份认证凭据
/// - `uuid`: 设备 UUID
/// - 返回: 设备编号
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
pub(super) async fn authorize_and_resolve(token: &str, uuid: Uuid) -> anyhow::Result<i16> {
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
    let is_allowed = check_token_limit(
        &token_or_auth,
        &[Scope::AgentUuid(uuid)],
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
        .get_id(&uuid)
        .ok_or_else(|| NodegetError::NotFound(format!("Unknown agent UUID: {uuid}")))?;
    Ok(uuid_id)
}

/// 检查开始时间不晚于结束时间。
///
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
///
/// # Errors
///
/// - 两者都填写且开始时间晚于结束时间时返回 `NodegetError::InvalidInput`
pub(super) fn check_time_order(
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<()> {
    if let (Some(start_time), Some(end_time)) = (start_time, end_time)
        && start_time > end_time
    {
        return Err(NodegetError::InvalidInput(
            "start_time must not be later than end_time".to_owned(),
        )
        .into());
    }
    Ok(())
}

/// 查询与时间段有重叠的可能丢失数据的时间段。
///
/// - `db`: 数据库连接
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 按开始时间排序
pub(super) async fn query_possible_data_losses(
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
pub(super) fn database_error(e: &DbErr) -> NodegetError {
    NodegetError::DatabaseError(format!("traffic query: {e}"))
}

/// 流量查询测试共用的数据。
#[cfg(test)]
pub(super) mod test_support {
    use crate::traffic_stats::traffic_tables_on_sqlite;
    use ng_db::entity::{traffic_current_total, traffic_possible_data_loss, traffic_snapshot};
    use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait, Set};

    pub const MINUTE: i64 = 60 * 1000;
    /// 2026-09-27 10:00:00 UTC
    pub const TEN_OCLOCK: i64 = 1_790_503_200_000;

    /// 某个时刻：10 点过几分
    pub const fn at(minutes: i64) -> i64 {
        TEN_OCLOCK + minutes * MINUTE
    }

    /// 准备测试数据：
    /// - eth0：10:00 快照 100，10:15 快照 150，10:30 快照 230，当前总流量 260
    /// - eth1：10:15 快照 10，当前总流量 20
    /// - 可能丢失数据：10:20 到 10:40
    /// - 另一台设备（编号 2）的数据不应被查到
    pub async fn seeded_db() -> DatabaseConnection {
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
}

#[cfg(test)]
mod tests {
    use super::test_support::{at, seeded_db};
    use super::*;

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

    #[test]
    fn start_time_later_than_end_time_is_rejected() {
        assert!(check_time_order(Some(2), Some(1)).is_err());
        assert!(check_time_order(Some(1), Some(1)).is_ok());
        assert!(check_time_order(Some(1), None).is_ok());
        assert!(check_time_order(None, Some(1)).is_ok());
    }
}
