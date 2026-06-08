//! `agent.query_dynamic` RPC 实现。
//!
//! 按条件查询动态监控数据，支持字段选择、UUID/时间戳/入库时间过滤、Limit/Last。
//! 使用流式查询（stream）逐行处理，避免一次性加载全部数据到内存。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{DynamicDataQuery, DynamicDataQueryField, QueryCondition};
use crate::rpc::agent::AgentRpcImpl;
use futures_util::StreamExt;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::error_message::anyhow_error_to_raw;
use ng_core::utils::server_json::rename_and_fix_json;
use ng_db::entity::dynamic_monitoring;
use ng_infra::server::RpcHelper;
use ng_token::get::check_token_limit;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder,
    QuerySelect, SelectModel, Selector,
};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, error, warn};

/// 查询动态监控数据。
///
/// - `token` — 身份认证凭据
/// - `dynamic_data_query` — 查询参数（字段 + 条件）
/// - 返回值 — 匹配记录的 JSON 数组
///
/// 内部步骤：
/// 1. 解析 Token 并按字段粒度验证 `DynamicMonitoring::Read` 权限
/// 2. 构建 `SeaORM` 查询（`select_only` + 字段映射 + 条件过滤）
/// 3. UUID 条件通过缓存转换为 `uuid_id`
/// 4. 应用排序和 Limit（默认 10000，上限 10000）
/// 5. 流式执行查询，逐行转换 `uuid_id`→`uuid`、重命名字段
/// 6. 手动拼接 JSON 数组字符串，返回 `RawValue`
#[allow(clippy::too_many_lines)]
pub async fn query_dynamic(
    token: String,
    dynamic_data_query: DynamicDataQuery,
) -> RpcResult<Box<RawValue>> {
    const DEFAULT_LIMIT: u64 = 10_000;
    const MAX_LIMIT: u64 = 10_000;
    let process_logic = async {
        debug!(target: "monitoring", conditions_count = dynamic_data_query.condition.len(), fields_count = dynamic_data_query.fields.len(), "Dynamic query request received");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;

        for cond in &dynamic_data_query.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }

        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let is_allowed = if dynamic_data_query.fields.is_empty() {
            let mut any_allowed = false;
            for permission in [
                Permission::DynamicMonitoring(DynamicMonitoring::Read(DynamicDataQueryField::Cpu)),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(DynamicDataQueryField::Ram)),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(DynamicDataQueryField::Load)),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(
                    DynamicDataQueryField::System,
                )),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(DynamicDataQueryField::Disk)),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(
                    DynamicDataQueryField::Network,
                )),
                Permission::DynamicMonitoring(DynamicMonitoring::Read(DynamicDataQueryField::Gpu)),
            ] {
                if check_token_limit(&token_or_auth, scopes.clone(), vec![permission]).await? {
                    any_allowed = true;
                    break;
                }
            }
            any_allowed
        } else {
            let permissions: Vec<Permission> = dynamic_data_query
                .fields
                .iter()
                .map(|field| Permission::DynamicMonitoring(DynamicMonitoring::Read(*field)))
                .collect();

            check_token_limit(&token_or_auth, scopes, permissions).await?
        };

        if !is_allowed {
            warn!(target: "monitoring", "权限拒绝: 缺少 DynamicMonitoring Read 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient DynamicMonitoring Read permissions".to_string(),
            )
            .into());
        }

        debug!(target: "monitoring", conditions_count = dynamic_data_query.condition.len(), fields_count = dynamic_data_query.fields.len(), "Dynamic query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned()))?;

        let query = dynamic_monitoring::Entity::find()
            .select_only()
            .column(dynamic_monitoring::Column::UuidId)
            .column(dynamic_monitoring::Column::Timestamp);

        let query = dynamic_data_query
            .fields
            .iter()
            .fold(query, |q, field| match field {
                DynamicDataQueryField::Cpu => q.column(dynamic_monitoring::Column::CpuData),
                DynamicDataQueryField::Ram => q.column(dynamic_monitoring::Column::RamData),
                DynamicDataQueryField::Load => q.column(dynamic_monitoring::Column::LoadData),
                DynamicDataQueryField::System => q.column(dynamic_monitoring::Column::SystemData),
                DynamicDataQueryField::Disk => q.column(dynamic_monitoring::Column::DiskData),
                DynamicDataQueryField::Network => q.column(dynamic_monitoring::Column::NetworkData),
                DynamicDataQueryField::Gpu => q.column(dynamic_monitoring::Column::GpuData),
            });

        let mut limit_count = None;
        let mut is_last = false;

        let mut uuid_ids: Vec<i16> = Vec::new();
        for cond in &dynamic_data_query.condition {
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

        let query = dynamic_data_query
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(_) => {
                    let uuid_id = uuid_id_iter.next().unwrap();
                    q.filter(dynamic_monitoring::Column::UuidId.eq(uuid_id))
                }
                QueryCondition::TimestampFromTo(start, end) => q.filter(
                    dynamic_monitoring::Column::Timestamp
                        .gte(start)
                        .and(dynamic_monitoring::Column::Timestamp.lte(end)),
                ),
                QueryCondition::TimestampFrom(start) => {
                    q.filter(dynamic_monitoring::Column::Timestamp.gte(start))
                }
                QueryCondition::TimestampTo(end) => {
                    q.filter(dynamic_monitoring::Column::Timestamp.lte(end))
                }
                QueryCondition::StorageTimeFromTo(start, end) => q.filter(
                    dynamic_monitoring::Column::StorageTime
                        .gte(start)
                        .and(dynamic_monitoring::Column::StorageTime.lte(end)),
                ),
                QueryCondition::StorageTimeFrom(start) => {
                    q.filter(dynamic_monitoring::Column::StorageTime.gte(start))
                }
                QueryCondition::StorageTimeTo(end) => {
                    q.filter(dynamic_monitoring::Column::StorageTime.lte(end))
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
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = clamped_limit {
            query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Asc)
                .limit(DEFAULT_LIMIT)
        };

        let field_mappings: Vec<(&str, &str)> = dynamic_data_query
            .fields
            .iter()
            .map(|f| (f.column_name(), f.json_key()))
            .collect();

        execute_query(
            db,
            query.into_json(),
            &field_mappings,
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

/// 流式执行动态监控数据查询，逐行处理并拼接 JSON 数组。
///
/// - `db` — 数据库连接
/// - `query` — `SeaORM` `Selector`
/// - `field_mappings` — 列名→JSON 键名映射
/// - `capacity_hint` — 预估结果行数，用于预分配缓冲区
/// - `uuid_cache` — UUID 缓存，用于 `uuid_id`→`uuid` 转换
/// - 返回值 — JSON 数组的 `RawValue`
async fn execute_query(
    db: &DatabaseConnection,
    query: Selector<SelectModel<serde_json::Value>>,
    field_mappings: &[(&str, &str)],
    capacity_hint: u64,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    debug!(target: "monitoring", "Starting dynamic query DB stream");
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
                if let Some(obj) = v.as_object_mut() {
                    if let Some(Value::Number(n)) = obj.remove("uuid_id")
                        && let Some(id) = n.as_i64()
                        && let Some(uuid) = uuid_cache.get_uuid(id as i16)
                    {
                        obj.insert("uuid".to_string(), Value::String(uuid.to_string()));
                    }
                    for (old_key, new_key) in field_mappings {
                        rename_and_fix_json(obj, old_key, new_key);
                    }
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

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring query completed");

    Ok(raw_value)
}
