//! `agent.query_dynamic_summary` RPC 实现。
//!
//! 按条件查询动态摘要监控数据，支持字段选择、UUID/时间戳/入库时间过滤、Limit/Last。
//! 查询结果中对缩放列（`SCALED_SUMMARY_COLUMNS`）执行反缩放处理。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{DynamicSummaryQuery, DynamicSummaryQueryField, QueryCondition};
use crate::rpc::agent::AgentRpcImpl;
use futures_util::StreamExt;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoringSummary, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::error_message::anyhow_error_to_raw;
use ng_db::entity::dynamic_monitoring_summary;
use ng_infra::server::RpcHelper;
use ng_token::get::check_token_limit;
use sea_orm::{
    ColumnTrait, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect, SelectModel,
    Selector,
};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, error, warn};

/// 查询动态摘要监控数据。
///
/// - `token` — 身份认证凭据
/// - `query_data` — 查询参数（字段 + 条件）
/// - 返回值 — 匹配记录的 JSON 数组（缩放列已反缩放）
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoringSummary::Read` 权限
/// 2. 构建 `SeaORM` 查询（`select_only` + 字段映射 + 条件过滤）
/// 3. UUID 条件通过缓存转换为 `uuid_id`
/// 4. 应用排序和 Limit
/// 5. 流式执行查询，逐行执行 `uuid_id`→`uuid` 转换和反缩放处理
/// 6. 手动拼接 JSON 数组字符串，返回 `RawValue`
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
/// - 序列化失败时返回 `NodegetError::SerializationError`
///
/// # Panics
///
/// 内部使用 `uuid_id_iter.next().unwrap()`，若 UUID 条件数量与缓存解析结果不一致则 panic（理论上不会发生）。
#[allow(clippy::too_many_lines)]
pub async fn query_dynamic_summary(
    token: String,
    query_data: DynamicSummaryQuery,
) -> RpcResult<Box<RawValue>> {
    const DEFAULT_LIMIT: u64 = 10_000;
    const MAX_LIMIT: u64 = 10_000;
    let process_logic = async {
        debug!(target: "monitoring", conditions_count = query_data.condition.len(), fields_count = query_data.fields.len(), "Dynamic summary query request received");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;

        for cond in &query_data.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }

        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let is_allowed = check_token_limit(
            &token_or_auth,
            &scopes,
            &[Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Read,
            )],
        )
        .await?;

        if !is_allowed {
            warn!(target: "monitoring", "权限拒绝: 缺少 DynamicMonitoringSummary Read 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Read permission".to_string(),
            )
            .into());
        }

        debug!(target: "monitoring", conditions_count = query_data.condition.len(), fields_count = query_data.fields.len(), "Dynamic summary query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
        })?;

        let query = dynamic_monitoring_summary::Entity::find()
            .select_only()
            .column(dynamic_monitoring_summary::Column::UuidId)
            .column(dynamic_monitoring_summary::Column::Timestamp);

        let query = if query_data.fields.is_empty() {
            query
                .column(dynamic_monitoring_summary::Column::CpuUsage)
                .column(dynamic_monitoring_summary::Column::GpuUsage)
                .column(dynamic_monitoring_summary::Column::UsedSwap)
                .column(dynamic_monitoring_summary::Column::TotalSwap)
                .column(dynamic_monitoring_summary::Column::UsedMemory)
                .column(dynamic_monitoring_summary::Column::TotalMemory)
                .column(dynamic_monitoring_summary::Column::AvailableMemory)
                .column(dynamic_monitoring_summary::Column::LoadOne)
                .column(dynamic_monitoring_summary::Column::LoadFive)
                .column(dynamic_monitoring_summary::Column::LoadFifteen)
                .column(dynamic_monitoring_summary::Column::Uptime)
                .column(dynamic_monitoring_summary::Column::BootTime)
                .column(dynamic_monitoring_summary::Column::ProcessCount)
                .column(dynamic_monitoring_summary::Column::TotalSpace)
                .column(dynamic_monitoring_summary::Column::AvailableSpace)
                .column(dynamic_monitoring_summary::Column::ReadSpeed)
                .column(dynamic_monitoring_summary::Column::WriteSpeed)
                .column(dynamic_monitoring_summary::Column::TcpConnections)
                .column(dynamic_monitoring_summary::Column::UdpConnections)
                .column(dynamic_monitoring_summary::Column::TotalReceived)
                .column(dynamic_monitoring_summary::Column::TotalTransmitted)
                .column(dynamic_monitoring_summary::Column::TransmitSpeed)
                .column(dynamic_monitoring_summary::Column::ReceiveSpeed)
        } else {
            query_data
                .fields
                .iter()
                .fold(query, |q, field| q.column(field_to_column(field)))
        };

        let mut limit_count = None;
        let mut is_last = false;

        let mut uuid_ids: Vec<i16> = Vec::new();
        for cond in &query_data.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                let uuid_id = uuid_cache.get_id(uuid).ok_or_else(|| {
                    anyhow::anyhow!(NodegetError::NotFound(format!(
                        "Unknown agent UUID: {uuid}"
                    )))
                })?;
                uuid_ids.push(uuid_id);
            }
        }
        let mut uuid_id_iter = uuid_ids.into_iter();

        let query = query_data
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(_) => {
                    let uuid_id = uuid_id_iter.next().unwrap();
                    q.filter(dynamic_monitoring_summary::Column::UuidId.eq(uuid_id))
                }
                QueryCondition::TimestampFromTo(start, end) => q.filter(
                    dynamic_monitoring_summary::Column::Timestamp
                        .gte(start)
                        .and(dynamic_monitoring_summary::Column::Timestamp.lte(end)),
                ),
                QueryCondition::TimestampFrom(start) => {
                    q.filter(dynamic_monitoring_summary::Column::Timestamp.gte(start))
                }
                QueryCondition::TimestampTo(end) => {
                    q.filter(dynamic_monitoring_summary::Column::Timestamp.lte(end))
                }
                QueryCondition::StorageTimeFromTo(start, end) => q.filter(
                    dynamic_monitoring_summary::Column::StorageTime
                        .gte(start)
                        .and(dynamic_monitoring_summary::Column::StorageTime.lte(end)),
                ),
                QueryCondition::StorageTimeFrom(start) => {
                    q.filter(dynamic_monitoring_summary::Column::StorageTime.gte(start))
                }
                QueryCondition::StorageTimeTo(end) => {
                    q.filter(dynamic_monitoring_summary::Column::StorageTime.lte(end))
                }
                QueryCondition::Limit(n) => {
                    limit_count = Some(n);
                    q
                }
                QueryCondition::Last => {
                    is_last = true;
                    q
                }
            });

        let clamped_limit = limit_count.map(|l| std::cmp::min(l, MAX_LIMIT));

        let query = if is_last {
            query
                .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = clamped_limit {
            query
                .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query
                .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Asc)
                .limit(DEFAULT_LIMIT)
        };

        execute_query(
            db,
            query.into_json(),
            clamped_limit.unwrap_or(5000),
            uuid_cache,
        )
        .await
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let raw = anyhow_error_to_raw(&e).unwrap_or_else(|_| {
                RawValue::from_string(
                    r#"{"error_id":999,"error_message":"Internal error"}"#.to_string(),
                )
                .unwrap_or_else(|_| RawValue::from_string("null".to_string()).unwrap())
            });
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            let json_str = raw.get();
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                Some(json_str),
            ))
        }
    }
}

/// 将查询字段映射到对应的 Entity 列。
///
/// 公开是因为 `query_dynamic_summary_multi_last` 复用同一映射。
#[must_use]
pub const fn field_to_column(
    field: &DynamicSummaryQueryField,
) -> dynamic_monitoring_summary::Column {
    match field {
        DynamicSummaryQueryField::CpuUsage => dynamic_monitoring_summary::Column::CpuUsage,
        DynamicSummaryQueryField::GpuUsage => dynamic_monitoring_summary::Column::GpuUsage,
        DynamicSummaryQueryField::UsedSwap => dynamic_monitoring_summary::Column::UsedSwap,
        DynamicSummaryQueryField::TotalSwap => dynamic_monitoring_summary::Column::TotalSwap,
        DynamicSummaryQueryField::UsedMemory => dynamic_monitoring_summary::Column::UsedMemory,
        DynamicSummaryQueryField::TotalMemory => dynamic_monitoring_summary::Column::TotalMemory,
        DynamicSummaryQueryField::AvailableMemory => {
            dynamic_monitoring_summary::Column::AvailableMemory
        }
        DynamicSummaryQueryField::LoadOne => dynamic_monitoring_summary::Column::LoadOne,
        DynamicSummaryQueryField::LoadFive => dynamic_monitoring_summary::Column::LoadFive,
        DynamicSummaryQueryField::LoadFifteen => dynamic_monitoring_summary::Column::LoadFifteen,
        DynamicSummaryQueryField::Uptime => dynamic_monitoring_summary::Column::Uptime,
        DynamicSummaryQueryField::BootTime => dynamic_monitoring_summary::Column::BootTime,
        DynamicSummaryQueryField::ProcessCount => dynamic_monitoring_summary::Column::ProcessCount,
        DynamicSummaryQueryField::TotalSpace => dynamic_monitoring_summary::Column::TotalSpace,
        DynamicSummaryQueryField::AvailableSpace => {
            dynamic_monitoring_summary::Column::AvailableSpace
        }
        DynamicSummaryQueryField::ReadSpeed => dynamic_monitoring_summary::Column::ReadSpeed,
        DynamicSummaryQueryField::WriteSpeed => dynamic_monitoring_summary::Column::WriteSpeed,
        DynamicSummaryQueryField::TcpConnections => {
            dynamic_monitoring_summary::Column::TcpConnections
        }
        DynamicSummaryQueryField::UdpConnections => {
            dynamic_monitoring_summary::Column::UdpConnections
        }
        DynamicSummaryQueryField::TotalReceived => {
            dynamic_monitoring_summary::Column::TotalReceived
        }
        DynamicSummaryQueryField::TotalTransmitted => {
            dynamic_monitoring_summary::Column::TotalTransmitted
        }
        DynamicSummaryQueryField::TransmitSpeed => {
            dynamic_monitoring_summary::Column::TransmitSpeed
        }
        DynamicSummaryQueryField::ReceiveSpeed => dynamic_monitoring_summary::Column::ReceiveSpeed,
    }
}

/// 流式执行动态摘要查询，逐行处理（`uuid_id`→`uuid` + 反缩放）并拼接 JSON 数组。
///
/// - `db` — 数据库连接
/// - `query` — `SeaORM` `Selector`
/// - `capacity_hint` — 预估结果行数
/// - `uuid_cache` — UUID 缓存
/// - 返回值 — JSON 数组的 `RawValue`
async fn execute_query(
    db: &sea_orm::DatabaseConnection,
    query: Selector<SelectModel<serde_json::Value>>,
    capacity_hint: u64,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    debug!(target: "monitoring", "Starting dynamic summary query DB stream");
    let mut stream = query.stream(db).await.map_err(|e| {
        error!(target: "monitoring", error = %e, "Database query error");
        NodegetError::DatabaseError(format!("Database query error: {e}"))
    })?;

    let capacity = (capacity_hint as usize).saturating_mul(200);
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;
    let mut result_count: usize = 0;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut v) => {
                result_count += 1;

                if let Value::Object(ref mut map) = v {
                    if let Some(Value::Number(n)) = map.remove("uuid_id")
                        && let Some(id) = n.as_i64()
                        && let Some(uuid) = uuid_cache.get_uuid(id as i16)
                    {
                        map.insert("uuid".to_string(), Value::String(uuid.to_string()));
                    }
                    crate::query::apply_descaling_to_json_object(map);
                }

                if first {
                    first = false;
                } else {
                    output_buffer.push(b',');
                }

                if let Err(e) = serde_json::to_writer(&mut output_buffer, &v) {
                    error!(target: "monitoring", error = %e, "Serialization failed");
                    return Err(NodegetError::SerializationError(format!(
                        "Serialization failed: {e}"
                    ))
                    .into());
                }
            }
            Err(e) => {
                error!(target: "monitoring", error = %e, "Stream read error");
                return Err(NodegetError::DatabaseError(format!("Stream read error: {e}")).into());
            }
        }
    }

    output_buffer.push(b']');

    let json_string = String::from_utf8(output_buffer).map_err(|e| {
        error!(target: "monitoring", error = %e, "UTF8 conversion error");
        NodegetError::SerializationError("UTF8 conversion error (internal)".to_string())
    })?;

    let raw_value = RawValue::from_string(json_string).map_err(|e| {
        error!(target: "monitoring", error = %e, "RawValue creation error");
        NodegetError::SerializationError("RawValue creation error".to_string())
    })?;

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring summary query completed");

    Ok(raw_value)
}
