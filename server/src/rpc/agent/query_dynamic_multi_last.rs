use crate::entity::dynamic_monitoring;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::DynamicDataQueryField;
use nodeget_lib::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use nodeget_lib::utils::server_json::rename_and_fix_json;
use sea_orm::sea_query::{Alias, Query, SelectStatement, UnionType};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, Order, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Statement, StatementBuilder,
};
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, error};
use uuid::Uuid;

pub async fn dynamic_data_multi_last_query(
    token: String,
    uuids: Vec<Uuid>,
    fields: Vec<DynamicDataQueryField>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "monitoring", uuids_count = uuids.len(), fields_count = fields.len(), "Dynamic multi-last query request received");

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
            let permissions: Vec<Permission> = fields
                .iter()
                .map(|field| Permission::DynamicMonitoring(DynamicMonitoring::Read(*field)))
                .collect();
            check_token_limit(&token_or_auth, scopes, permissions).await?
        };

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient DynamicMonitoring Read permissions".to_owned(),
            )
            .into());
        }

        debug!(target: "monitoring", uuids_count = deduped_uuids.len(), fields_count = fields.len(), "Dynamic multi-last query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global();

        // Resolve UUIDs to uuid_ids
        let mut uuid_id_pairs: Vec<(Uuid, i16)> = Vec::with_capacity(deduped_uuids.len());
        for uuid in &deduped_uuids {
            let uuid_id = uuid_cache.get_id(uuid).await.ok_or_else(|| {
                NodegetError::NotFound(format!("Agent UUID not found in monitoring registry: {uuid}"))
            })?;
            uuid_id_pairs.push((*uuid, uuid_id));
        }

        let statement = build_union_last_statement(&uuid_id_pairs, &fields, db)?;

        let field_mappings: Vec<(&str, &str)> = fields
            .iter()
            .map(|field| (field.column_name(), field.json_key()))
            .collect();

        execute_statement_query(db, statement, &field_mappings, deduped_uuids.len(), &uuid_cache).await
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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            let json_str = raw.get();
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                Some(json_str),
            ))
        }
    }
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
    fields: &[DynamicDataQueryField],
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

fn build_single_last_select(uuid_id: i16, fields: &[DynamicDataQueryField]) -> SelectStatement {
    let inner_query = dynamic_monitoring::Entity::find()
        .select_only()
        .column(dynamic_monitoring::Column::UuidId)
        .column(dynamic_monitoring::Column::Timestamp);

    let inner_query = fields.iter().fold(inner_query, |query, field| match field {
        DynamicDataQueryField::Cpu => query.column(dynamic_monitoring::Column::CpuData),
        DynamicDataQueryField::Ram => query.column(dynamic_monitoring::Column::RamData),
        DynamicDataQueryField::Load => query.column(dynamic_monitoring::Column::LoadData),
        DynamicDataQueryField::System => query.column(dynamic_monitoring::Column::SystemData),
        DynamicDataQueryField::Disk => query.column(dynamic_monitoring::Column::DiskData),
        DynamicDataQueryField::Network => query.column(dynamic_monitoring::Column::NetworkData),
        DynamicDataQueryField::Gpu => query.column(dynamic_monitoring::Column::GpuData),
    });

    let inner_query = inner_query
        .filter(dynamic_monitoring::Column::UuidId.eq(uuid_id))
        .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
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
            DynamicDataQueryField::Cpu => {
                wrapped.column((alias.clone(), Alias::new("cpu_data")));
            }
            DynamicDataQueryField::Ram => {
                wrapped.column((alias.clone(), Alias::new("ram_data")));
            }
            DynamicDataQueryField::Load => {
                wrapped.column((alias.clone(), Alias::new("load_data")));
            }
            DynamicDataQueryField::System => {
                wrapped.column((alias.clone(), Alias::new("system_data")));
            }
            DynamicDataQueryField::Disk => {
                wrapped.column((alias.clone(), Alias::new("disk_data")));
            }
            DynamicDataQueryField::Network => {
                wrapped.column((alias.clone(), Alias::new("network_data")));
            }
            DynamicDataQueryField::Gpu => {
                wrapped.column((alias.clone(), Alias::new("gpu_data")));
            }
        }
    }

    wrapped.clone()
}

async fn execute_statement_query(
    db: &DatabaseConnection,
    statement: Statement,
    field_mappings: &[(&str, &str)],
    capacity_hint: usize,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    debug!(target: "monitoring", "Starting dynamic multi-last query DB stream");
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
                    // Translate uuid_id → uuid string
                    if let Some(uuid_id_val) = obj.remove("uuid_id") {
                        if let Some(uuid_id) = uuid_id_val.as_i64() {
                            if let Some(uuid) = uuid_cache.get_uuid(uuid_id as i16).await {
                                obj.insert("uuid".to_owned(), serde_json::Value::String(uuid.to_string()));
                            }
                        }
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

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring multi-last query completed");

    Ok(raw_value)
}
