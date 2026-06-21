//! `agent.static_data_multi_last_query` RPC 实现。
//!
//! 批量查询多台设备的静态监控最新值。优先从内存 last-cache 获取，
//! 缓存未命中的部分通过 UNION ALL 查询数据库，最后合并结果。
//!
//! ## 性能优化
//!
//! 全字段查询（请求所有 3 个字段）使用 `get_static_last_raw()` 直接获取
//! 预序列化字符串（`Arc<str>`），跳过 Map 构造、键分配和 Value 克隆。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::StaticDataQueryField;
use crate::rpc::agent::AgentRpcImpl;
use futures_util::StreamExt;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, StaticMonitoring};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::error_message::anyhow_error_to_raw;
use ng_core::utils::server_json::rename_and_fix_json;
use ng_db::entity::static_monitoring;
use ng_infra::server::RpcHelper;
use ng_token::get::check_token_limit;
use sea_orm::sea_query::{Alias, Query, SelectStatement, UnionType};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, Order, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Statement, StatementBuilder,
};
use serde_json::value::RawValue;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, error, warn};
use uuid::Uuid;

/// 静态监控全部可查询字段，用于判断是否为全字段查询。
const ALL_STATIC_FIELDS: &[StaticDataQueryField] = &[
    StaticDataQueryField::Cpu,
    StaticDataQueryField::System,
    StaticDataQueryField::Gpu,
];

/// 查询结果的统一表示，支持零拷贝和序列化两种来源。
#[derive(Clone)]
enum StaticResult {
    /// 预序列化的 JSON 字符串（全字段缓存命中）
    Raw(Arc<str>),
    /// 需要序列化的 JSON Value（筛选字段或 DB 结果）
    Value(serde_json::Value),
}

/// 批量查询多台设备的静态监控最新值。
///
/// - `token` — 身份认证凭据
/// - `uuids` — 设备 UUID 列表（自动去重）
/// - `fields` — 需要的字段列表
/// - 返回值 — 最新记录的 JSON 数组
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `StaticMonitoring::Read` 权限
/// 2. UUID 去重并通过缓存转换为 `uuid_id`
/// 3. 优先从 `MonitoringLastCache` 获取缓存命中
/// 4. 缓存未命中的部分构建 UNION ALL 语句查询 DB
/// 5. 合并缓存与 DB 结果，序列化返回
#[allow(clippy::too_many_lines)]
pub async fn static_data_multi_last_query(
    token: String,
    uuids: Vec<Uuid>,
    fields: Vec<StaticDataQueryField>,
) -> RpcResult<Box<RawValue>> {
    let is_all_fields = is_all_static_fields(&fields);
    let process_logic = async {
        debug!(target: "monitoring", uuids_count = uuids.len(), fields_count = fields.len(), "Static multi-last query request received");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let deduped_uuids = dedupe_uuids(uuids);
        if deduped_uuids.is_empty() {
            return RawValue::from_string("[]".to_owned())
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        let scopes: Vec<Scope> = deduped_uuids
            .iter()
            .map(|uuid| Scope::AgentUuid(*uuid))
            .collect();

        let is_allowed = if fields.is_empty() {
            let mut any_allowed = false;
            for permission in [
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::Cpu)),
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::System)),
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::Gpu)),
            ] {
                if check_token_limit(&token_or_auth, &scopes, &[permission]).await? {
                    any_allowed = true;
                    break;
                }
            }
            any_allowed
        } else {
            let permissions: Vec<Permission> = fields
                .iter()
                .map(|field| Permission::StaticMonitoring(StaticMonitoring::Read(*field)))
                .collect();
            check_token_limit(&token_or_auth, &scopes, &permissions).await?
        };

        if !is_allowed {
            warn!(target: "monitoring", "权限拒绝: 缺少 StaticMonitoring Read 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient StaticMonitoring Read permissions".to_owned(),
            )
            .into());
        }

        debug!(target: "monitoring", uuids_count = deduped_uuids.len(), fields_count = fields.len(), "Static multi-last query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
        })?;

        // Resolve UUIDs to uuid_ids
        let mut uuid_id_pairs: Vec<(Uuid, i16)> = Vec::with_capacity(deduped_uuids.len());
        for uuid in &deduped_uuids {
            let uuid_id = uuid_cache.get_id(uuid).ok_or_else(|| {
                NodegetError::NotFound(format!(
                    "Agent UUID not found in monitoring registry: {uuid}"
                ))
            })?;
            uuid_id_pairs.push((*uuid, uuid_id));
        }

        // ── Fast path: in-memory last-cache (partial hit merge) ─────
        let last_cache =
            crate::monitoring_last_cache::MonitoringLastCache::global().ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringLastCache not initialized".to_owned())
            })?;
        let mut results: Vec<Option<StaticResult>> = vec![None; uuid_id_pairs.len()];
        let mut misses: Vec<(usize, i16)> = Vec::new();
        for (idx, (uuid, uuid_id)) in uuid_id_pairs.iter().enumerate() {
            if is_all_fields {
                // 全字段查询：使用预序列化字符串，跳过 Map 构造和 Value 克隆
                match last_cache.get_static_last_raw(uuid) {
                    Some(s) => results[idx] = Some(StaticResult::Raw(s)),
                    None => misses.push((idx, *uuid_id)),
                }
            } else {
                // 筛选字段查询：使用 Value 路径
                match last_cache.get_static_last(uuid, &fields) {
                    Some(v) => results[idx] = Some(StaticResult::Value(v)),
                    None => misses.push((idx, *uuid_id)),
                }
            }
        }

        if misses.is_empty() {
            debug!(target: "monitoring", uuids_count = uuid_id_pairs.len(), "Static multi-last query full cache hit");
        } else {
            let miss_pairs: Vec<(Uuid, i16)> =
                misses.iter().map(|(idx, _)| uuid_id_pairs[*idx]).collect();
            let statement = build_union_last_statement(&miss_pairs, &fields, db)?;
            let field_mappings: Vec<(&str, &str)> = fields
                .iter()
                .map(|field| (field.column_name(), field.json_key()))
                .collect();
            let miss_values: Vec<serde_json::Value> = execute_statement_query(
                db,
                statement,
                &field_mappings,
                miss_pairs.len(),
                uuid_cache,
            )
            .await?;
            // 用 zip 配对，防御 DB 返回行数 < misses.len() 时的越界 panic。
            for (val, (idx, _)) in miss_values.into_iter().zip(misses.iter()) {
                results[*idx] = Some(StaticResult::Value(val));
            }
            debug!(target: "monitoring", cache_hits = uuid_id_pairs.len() - misses.len(), misses = misses.len(), "Static multi-last query partial cache hit");
        }

        // ── Unified serialization (cache + DB merged) ─────────────────
        let mut output_buffer: Vec<u8> = Vec::with_capacity(results.len().saturating_mul(200));
        output_buffer.push(b'[');
        let mut first = true;
        for result in results.into_iter().flatten() {
            if first {
                first = false;
            } else {
                output_buffer.push(b',');
            }
            match result {
                StaticResult::Raw(s) => output_buffer.extend_from_slice(s.as_bytes()),
                StaticResult::Value(v) => {
                    if let Err(e) = serde_json::to_writer(&mut output_buffer, &v) {
                        error!(target: "monitoring", error = %e, "Result serialization failed");
                        return Err(NodegetError::SerializationError(format!(
                            "Serialization failed: {e}"
                        ))
                        .into());
                    }
                }
            }
        }
        output_buffer.push(b']');
        let json_string = String::from_utf8(output_buffer)
            .map_err(|e| NodegetError::SerializationError(format!("UTF8 conversion error: {e}")))?;
        RawValue::from_string(json_string)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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

/// 判断是否请求了所有静态监控字段。
///
/// 使用集合等价判断，拒绝重复字段（如 [Cpu, Cpu, System] 不会被误判为全字段）。
fn is_all_static_fields(fields: &[StaticDataQueryField]) -> bool {
    let field_set: HashSet<_> = fields.iter().copied().collect();
    let all_set: HashSet<_> = ALL_STATIC_FIELDS.iter().copied().collect();
    field_set == all_set
}

/// UUID 列表去重，保持原始顺序。
fn dedupe_uuids(uuids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = HashSet::with_capacity(uuids.len());
    let mut deduped = Vec::with_capacity(uuids.len());

    for uuid in uuids {
        if seen.insert(uuid) {
            deduped.push(uuid);
        }
    }

    deduped
}

/// 为多台设备构建 UNION ALL 语句。
fn build_union_last_statement(
    uuid_id_pairs: &[(Uuid, i16)],
    fields: &[StaticDataQueryField],
    db: &DatabaseConnection,
) -> anyhow::Result<Statement> {
    let mut pair_iter = uuid_id_pairs.iter();
    let first_pair = pair_iter
        .next()
        .ok_or_else(|| NodegetError::InvalidInput("The uuids list cannot be empty".to_owned()))?;

    let mut union_query = build_single_last_select(first_pair.1, fields);
    for pair in pair_iter {
        union_query.union(UnionType::All, build_single_last_select(pair.1, fields));
    }

    Ok(StatementBuilder::build(
        &union_query,
        &db.get_database_backend(),
    ))
}

/// 为单台设备构建获取最新一条静态记录的 SELECT 语句。
fn build_single_last_select(uuid_id: i16, fields: &[StaticDataQueryField]) -> SelectStatement {
    let inner_query = static_monitoring::Entity::find()
        .select_only()
        .column(static_monitoring::Column::UuidId)
        .column(static_monitoring::Column::Timestamp);

    let inner_query = fields.iter().fold(inner_query, |query, field| match field {
        StaticDataQueryField::Cpu => query.column(static_monitoring::Column::CpuData),
        StaticDataQueryField::System => query.column(static_monitoring::Column::SystemData),
        StaticDataQueryField::Gpu => query.column(static_monitoring::Column::GpuData),
    });

    let inner_query = inner_query
        .filter(static_monitoring::Column::UuidId.eq(uuid_id))
        .order_by(static_monitoring::Column::Timestamp, Order::Desc)
        .limit(1)
        .into_query();

    let alias = Alias::new("last_row");
    let mut wrapped = Query::select();
    wrapped
        .column((alias.clone(), Alias::new("uuid_id")))
        .column((alias.clone(), Alias::new("timestamp")))
        .from_subquery(inner_query, alias.clone());

    for field in fields {
        match field {
            StaticDataQueryField::Cpu => {
                wrapped.column((alias.clone(), Alias::new("cpu_data")));
            }
            StaticDataQueryField::System => {
                wrapped.column((alias.clone(), Alias::new("system_data")));
            }
            StaticDataQueryField::Gpu => {
                wrapped.column((alias.clone(), Alias::new("gpu_data")));
            }
        }
    }

    wrapped.clone()
}

/// 流式执行 UNION ALL 语句查询，逐行处理并返回解析后的 JSON Value 列表。
///
/// 返回 `Vec<Value>` 而非序列化字符串，使调用方（miss 合并路径）无需
/// `from_str` 再解析一次，消除 serialize→parse→serialize 往返。
async fn execute_statement_query(
    db: &DatabaseConnection,
    statement: Statement,
    field_mappings: &[(&str, &str)],
    _capacity_hint: usize,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Vec<serde_json::Value>> {
    debug!(target: "monitoring", "Starting static multi-last query DB stream");
    let mut stream = serde_json::Value::find_by_statement(statement)
        .stream(db)
        .await
        .map_err(|e| {
            error!(target: "monitoring", error = %e, "Database query error");
            NodegetError::DatabaseError(format!("Database query error: {e}"))
        })?;

    let mut results: Vec<serde_json::Value> = Vec::new();

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut value) => {
                if let Some(obj) = value.as_object_mut() {
                    // Translate uuid_id → uuid string
                    if let Some(uuid_id_val) = obj.remove("uuid_id")
                        && let Some(uuid_id) = uuid_id_val.as_i64()
                        && let Some(uuid) = uuid_cache.get_uuid(uuid_id as i16)
                    {
                        obj.insert(
                            "uuid".to_owned(),
                            serde_json::Value::String(uuid.to_string()),
                        );
                    }
                    for (old_key, new_key) in field_mappings {
                        rename_and_fix_json(obj, old_key, new_key);
                    }
                }
                results.push(value);
            }
            Err(e) => {
                error!(target: "monitoring", error = %e, "Stream read error");
                return Err(NodegetError::DatabaseError(format!("Stream read error: {e}")).into());
            }
        }
    }

    debug!(target: "monitoring", result_count = results.len(), "Static monitoring multi-last query completed");

    Ok(results)
}
