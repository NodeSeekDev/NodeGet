use crate::monitoring_last_cache::MonitoringLastCache;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::query::{DynamicSummaryQueryField, apply_descaling_to_json_object};
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
use sea_orm::sea_query::{Alias, Query, SelectStatement, UnionType};
use sea_orm::{
    ColumnTrait, DatabaseBackend, DatabaseConnection, EntityTrait, FromQueryResult, Order,
    QueryFilter, QueryOrder, QuerySelect, QueryTrait, Statement, StatementBuilder,
};
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, error};
use uuid::Uuid;

use super::query_dynamic_summary::field_to_column;

/// All summary data column names for "select all" when fields is empty
const ALL_SUMMARY_COLUMNS: &[&str] = &[
    "cpu_usage",
    "gpu_usage",
    "used_swap",
    "total_swap",
    "used_memory",
    "total_memory",
    "available_memory",
    "load_one",
    "load_five",
    "load_fifteen",
    "uptime",
    "boot_time",
    "process_count",
    "total_space",
    "available_space",
    "read_speed",
    "write_speed",
    "tcp_connections",
    "udp_connections",
    "total_received",
    "total_transmitted",
    "transmit_speed",
    "receive_speed",
];

pub async fn dynamic_summary_multi_last_query(
    token: String,
    uuids: Vec<Uuid>,
    fields: Vec<DynamicSummaryQueryField>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "monitoring", uuids_count = uuids.len(), fields_count = fields.len(), "Dynamic summary multi-last query request received");

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

        let is_allowed = check_token_limit(
            &token_or_auth,
            scopes,
            vec![Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Read,
            )],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Read permission".to_owned(),
            )
            .into());
        }

        debug!(target: "monitoring", uuids_count = deduped_uuids.len(), fields_count = fields.len(), "Dynamic summary multi-last query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global();

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

        // Fast path: in-memory last-cache (partial hit merge)
        let last_cache = MonitoringLastCache::global();
        let mut results: Vec<Option<serde_json::Value>> = vec![None; uuid_id_pairs.len()];
        let mut misses: Vec<(usize, i16)> = Vec::new();
        for (idx, (uuid, uuid_id)) in uuid_id_pairs.iter().enumerate() {
            match last_cache.get_dynamic_summary_last(uuid, &fields) {
                Some(v) => {
                    results[idx] = Some(descale_cached_summary(v));
                }
                None => misses.push((idx, *uuid_id)),
            }
        }

        if misses.is_empty() {
            debug!(target: "monitoring", uuids_count = uuid_id_pairs.len(), "Dynamic summary multi-last query full cache hit");
        } else {
            let miss_pairs: Vec<(Uuid, i16)> =
                misses.iter().map(|(idx, _)| uuid_id_pairs[*idx]).collect();
            let statement = build_union_last_statement(&miss_pairs, &fields, db)?;
            let miss_raw =
                execute_statement_query(db, statement, miss_pairs.len(), uuid_cache).await?;
            let miss_values: Vec<serde_json::Value> = serde_json::from_str(miss_raw.get())
                .map_err(|e| NodegetError::SerializationError(format!("Parse DB results: {e}")))?;
            for (i, val) in miss_values.into_iter().enumerate() {
                let idx = misses[i].0;
                results[idx] = Some(val);
            }
            debug!(target: "monitoring", cache_hits = uuid_id_pairs.len() - misses.len(), misses = misses.len(), "Dynamic summary multi-last query partial cache hit");
        }

        // Unified serialization (cache + DB merged)
        let mut output_buffer: Vec<u8> = Vec::with_capacity(results.len().saturating_mul(200));
        output_buffer.push(b'[');
        let mut first = true;
        for value in results.into_iter().flatten() {
            if first {
                first = false;
            } else {
                output_buffer.push(b',');
            }
            if let Err(e) = serde_json::to_writer(&mut output_buffer, &value) {
                error!(target: "monitoring", error = %e, "Result serialization failed");
                return Err(
                    NodegetError::SerializationError(format!("Serialization failed: {e}")).into(),
                );
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

fn descale_cached_summary(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        apply_descaling_to_json_object(obj);
    }
    value
}

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

fn build_union_last_statement(
    uuid_id_pairs: &[(Uuid, i16)],
    fields: &[DynamicSummaryQueryField],
    db: &DatabaseConnection,
) -> anyhow::Result<Statement> {
    let backend = db.get_database_backend();
    let mut pair_iter = uuid_id_pairs.iter();
    let first_pair = pair_iter
        .next()
        .ok_or_else(|| NodegetError::InvalidInput("The uuids list cannot be empty".to_owned()))?;

    let mut union_query = build_single_last_select(first_pair.1, fields, backend);
    for pair in pair_iter {
        union_query.union(
            UnionType::All,
            build_single_last_select(pair.1, fields, backend),
        );
    }

    Ok(StatementBuilder::build(&union_query, &backend))
}

fn build_single_last_select(
    uuid_id: i16,
    fields: &[DynamicSummaryQueryField],
    backend: DatabaseBackend,
) -> SelectStatement {
    let _ = backend;

    let inner_query = dynamic_monitoring_summary::Entity::find()
        .select_only()
        .column(dynamic_monitoring_summary::Column::UuidId)
        .column(dynamic_monitoring_summary::Column::Timestamp);

    let inner_query = if fields.is_empty() {
        inner_query
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
        fields
            .iter()
            .fold(inner_query, |q, field| q.column(field_to_column(field)))
    };

    let inner_query = inner_query
        .filter(dynamic_monitoring_summary::Column::UuidId.eq(uuid_id))
        .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Desc)
        .limit(1)
        .into_query();

    let alias = Alias::new("last_row");
    let mut wrapped = Query::select();
    wrapped
        .column((alias.clone(), Alias::new("uuid_id")))
        .column((alias.clone(), Alias::new("timestamp")))
        .from_subquery(inner_query, alias.clone());

    let col_names: Vec<&str> = if fields.is_empty() {
        ALL_SUMMARY_COLUMNS.to_vec()
    } else {
        fields
            .iter()
            .map(DynamicSummaryQueryField::column_name)
            .collect()
    };

    for col_name in col_names {
        wrapped.column((alias.clone(), Alias::new(col_name)));
    }

    wrapped.clone()
}

async fn execute_statement_query(
    db: &DatabaseConnection,
    statement: Statement,
    capacity_hint: usize,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    debug!(target: "monitoring", "Starting dynamic summary multi-last query DB stream");
    let mut stream = serde_json::Value::find_by_statement(statement)
        .stream(db)
        .await
        .map_err(|e| {
            error!(target: "monitoring", error = %e, "Database query error");
            NodegetError::DatabaseError(format!("Database query error: {e}"))
        })?;

    let capacity = capacity_hint.saturating_mul(200);
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;
    let mut result_count: usize = 0;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut value) => {
                result_count += 1;
                if let Some(obj) = value.as_object_mut() {
                    if let Some(uuid_id_val) = obj.remove("uuid_id")
                        && let Some(uuid_id) = uuid_id_val.as_i64()
                        && let Some(uuid) = uuid_cache.get_uuid(uuid_id as i16)
                    {
                        obj.insert(
                            "uuid".to_owned(),
                            serde_json::Value::String(uuid.to_string()),
                        );
                    }
                    apply_descaling_to_json_object(obj);
                }
                if first {
                    first = false;
                } else {
                    output_buffer.push(b',');
                }

                if let Err(e) = serde_json::to_writer(&mut output_buffer, &value) {
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

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring summary multi-last query completed");

    Ok(raw_value)
}
