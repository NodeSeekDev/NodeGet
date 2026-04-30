use crate::entity::dynamic_monitoring_summary;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures_util::StreamExt;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::DynamicSummaryQueryField;
use nodeget_lib::permission::data_structure::{DynamicMonitoringSummary, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use sea_orm::sea_query::{Alias, Expr, Query, SelectStatement, UnionType};
use sea_orm::{
    ColumnTrait, DatabaseBackend, DatabaseConnection, EntityTrait, ExprTrait, FromQueryResult,
    Order, QueryFilter, QueryOrder, QuerySelect, QueryTrait, Statement, StatementBuilder,
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
            let uuid_id = uuid_cache.get_id(uuid).await.ok_or_else(|| {
                NodegetError::NotFound(format!(
                    "Agent UUID not found in monitoring registry: {uuid}"
                ))
            })?;
            uuid_id_pairs.push((*uuid, uuid_id));
        }

        let statement = build_union_last_statement(&uuid_id_pairs, &fields, db)?;

        execute_statement_query(db, statement, deduped_uuids.len(), uuid_cache).await
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

/// Column names that are stored as *10 scaled integers and need /10.0 on read
const SCALED_COLUMNS: &[&str] = &["cpu_usage", "load_one", "load_five", "load_fifteen"];

fn is_scaled_column(name: &str) -> bool {
    SCALED_COLUMNS.contains(&name)
}

fn build_single_last_select(
    uuid_id: i16,
    fields: &[DynamicSummaryQueryField],
    backend: DatabaseBackend,
) -> SelectStatement {
    let inner_query = dynamic_monitoring_summary::Entity::find()
        .select_only()
        .column(dynamic_monitoring_summary::Column::UuidId)
        .column(dynamic_monitoring_summary::Column::Timestamp);

    let inner_query = if fields.is_empty() {
        // Select all data columns
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
            .map(nodeget_lib::monitoring::query::DynamicSummaryQueryField::column_name)
            .collect()
    };

    if backend == DatabaseBackend::Postgres {
        // PostgreSQL handles expression aliases correctly in find_by_statement,
        // so we can descale directly in SQL.
        for col_name in col_names {
            if is_scaled_column(col_name) {
                wrapped.expr_as(
                    Expr::col((alias.clone(), Alias::new(col_name))).div(10.0),
                    Alias::new(col_name),
                );
            } else {
                wrapped.column((alias.clone(), Alias::new(col_name)));
            }
        }
    } else {
        // SQLite (and MySQL): expression aliases in raw subquery-to-JSON mapping
        // can drop columns, so we select raw values and descale in Rust code.
        for col_name in col_names {
            wrapped.column((alias.clone(), Alias::new(col_name)));
        }
    }

    wrapped.clone()
}

async fn execute_statement_query(
    db: &DatabaseConnection,
    statement: Statement,
    capacity_hint: usize,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    let needs_app_descaling = db.get_database_backend() != DatabaseBackend::Postgres;

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
                // Translate uuid_id → uuid string
                if let Some(obj) = value.as_object_mut() {
                    if let Some(uuid_id_val) = obj.remove("uuid_id")
                        && let Some(uuid_id) = uuid_id_val.as_i64()
                        && let Some(uuid) = uuid_cache.get_uuid(uuid_id as i16).await
                    {
                        obj.insert(
                            "uuid".to_owned(),
                            serde_json::Value::String(uuid.to_string()),
                        );
                    }
                    if needs_app_descaling {
                        apply_descaling(obj);
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

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring summary multi-last query completed");

    Ok(raw_value)
}

/// Apply /10.0 descaling to known scaled columns in the JSON object.
/// This is done in application code rather than SQL to work around
/// `SQLite` limitations with expression aliases in raw `find_by_statement` queries.
fn apply_descaling(obj: &mut serde_json::Map<String, serde_json::Value>) {
    const SCALED_FIELDS: &[&str] = &["cpu_usage", "load_one", "load_five", "load_fifteen"];
    for key in SCALED_FIELDS {
        if let Some(val) = obj.get_mut(*key)
            && let serde_json::Value::Number(n) = val {
                if let Some(i) = n.as_i64() {
                    if let Some(scaled) = serde_json::Number::from_f64(i as f64 / 10.0) {
                        *val = serde_json::Value::Number(scaled);
                    }
                } else if let Some(f) = n.as_f64()
                    && let Some(scaled) = serde_json::Number::from_f64(f / 10.0)
                {
                    *val = serde_json::Value::Number(scaled);
                }
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Schema, StatementBuilder};
    use serde_json::Value;

    #[tokio::test]
    async fn test_multi_last_sqlite_scaled_fields_present() {
        let db = Database::connect("sqlite::memory:").await.expect("connect");

        let schema = Schema::new(DatabaseBackend::Sqlite);
        let stmt = schema
            .create_table_from_entity(dynamic_monitoring_summary::Entity)
            .if_not_exists()
            .to_owned();
        db.execute(&stmt).await.expect("create table");

        let row = dynamic_monitoring_summary::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            uuid_id: sea_orm::ActiveValue::Set(1i16),
            timestamp: sea_orm::ActiveValue::Set(1_777_463_543_359i64),
            cpu_usage: sea_orm::ActiveValue::Set(Some(50i16)),
            gpu_usage: sea_orm::ActiveValue::Set(None),
            used_swap: sea_orm::ActiveValue::Set(Some(0i64)),
            total_swap: sea_orm::ActiveValue::Set(Some(0i64)),
            used_memory: sea_orm::ActiveValue::Set(Some(650_596_352i64)),
            total_memory: sea_orm::ActiveValue::Set(Some(16_734_150_656i64)),
            available_memory: sea_orm::ActiveValue::Set(Some(16_083_554_304i64)),
            load_one: sea_orm::ActiveValue::Set(Some(5i16)),
            load_five: sea_orm::ActiveValue::Set(Some(3i16)),
            load_fifteen: sea_orm::ActiveValue::Set(Some(1i16)),
            uptime: sea_orm::ActiveValue::Set(Some(14043i32)),
            boot_time: sea_orm::ActiveValue::Set(Some(1_777_449_499i64)),
            process_count: sea_orm::ActiveValue::Set(Some(135i32)),
            total_space: sea_orm::ActiveValue::Set(Some(64_353_267_200i64)),
            available_space: sea_orm::ActiveValue::Set(Some(61_662_812_160i64)),
            read_speed: sea_orm::ActiveValue::Set(Some(0i64)),
            write_speed: sea_orm::ActiveValue::Set(Some(35902i64)),
            tcp_connections: sea_orm::ActiveValue::Set(Some(14i32)),
            udp_connections: sea_orm::ActiveValue::Set(Some(2i32)),
            total_received: sea_orm::ActiveValue::Set(Some(52_957_882_012i64)),
            total_transmitted: sea_orm::ActiveValue::Set(Some(60_236_401_467i64)),
            transmit_speed: sea_orm::ActiveValue::Set(Some(8391i64)),
            receive_speed: sea_orm::ActiveValue::Set(Some(7160i64)),
        };
        dynamic_monitoring_summary::Entity::insert(row)
            .exec(&db)
            .await
            .expect("insert");

        let fields = vec![
            DynamicSummaryQueryField::CpuUsage,
            DynamicSummaryQueryField::UsedMemory,
            DynamicSummaryQueryField::TotalMemory,
            DynamicSummaryQueryField::AvailableMemory,
            DynamicSummaryQueryField::UsedSwap,
            DynamicSummaryQueryField::TotalSwap,
            DynamicSummaryQueryField::TotalSpace,
            DynamicSummaryQueryField::AvailableSpace,
            DynamicSummaryQueryField::ReadSpeed,
            DynamicSummaryQueryField::WriteSpeed,
            DynamicSummaryQueryField::ReceiveSpeed,
            DynamicSummaryQueryField::TransmitSpeed,
            DynamicSummaryQueryField::TotalReceived,
            DynamicSummaryQueryField::TotalTransmitted,
            DynamicSummaryQueryField::LoadOne,
            DynamicSummaryQueryField::LoadFive,
            DynamicSummaryQueryField::LoadFifteen,
            DynamicSummaryQueryField::Uptime,
            DynamicSummaryQueryField::BootTime,
            DynamicSummaryQueryField::ProcessCount,
            DynamicSummaryQueryField::TcpConnections,
            DynamicSummaryQueryField::UdpConnections,
        ];

        let statement = build_single_last_select(1i16, &fields, DatabaseBackend::Sqlite);
        let statement = StatementBuilder::build(&statement, &DatabaseBackend::Sqlite);

        let rows: Vec<Value> = Value::find_by_statement(statement)
            .all(&db)
            .await
            .expect("query");

        assert_eq!(rows.len(), 1);
        let row = rows.into_iter().next().unwrap();
        let mut obj = row.as_object().expect("object").clone();

        // Verify scaled fields are present in raw query result
        assert!(
            obj.contains_key("cpu_usage"),
            "cpu_usage must be present in result"
        );
        assert!(
            obj.contains_key("load_one"),
            "load_one must be present in result"
        );
        assert!(
            obj.contains_key("load_five"),
            "load_five must be present in result"
        );
        assert!(
            obj.contains_key("load_fifteen"),
            "load_fifteen must be present in result"
        );

        // Raw values are still scaled (stored as *10 integers)
        assert_eq!(obj["cpu_usage"], Value::Number(50i64.into()));
        assert_eq!(obj["load_one"], Value::Number(5i64.into()));
        assert_eq!(obj["load_five"], Value::Number(3i64.into()));
        assert_eq!(obj["load_fifteen"], Value::Number(1i64.into()));

        // Apply descaling and verify
        apply_descaling(&mut obj);
        assert_eq!(
            obj["cpu_usage"],
            Value::Number(serde_json::Number::from_f64(5.0).unwrap())
        );
        assert_eq!(
            obj["load_one"],
            Value::Number(serde_json::Number::from_f64(0.5).unwrap())
        );
        assert_eq!(
            obj["load_five"],
            Value::Number(serde_json::Number::from_f64(0.3).unwrap())
        );
        assert_eq!(
            obj["load_fifteen"],
            Value::Number(serde_json::Number::from_f64(0.1).unwrap())
        );

        // Verify other fields are unaffected
        assert_eq!(obj["used_memory"], Value::Number(650_596_352i64.into()));
    }

    #[test]
    fn test_postgres_sql_includes_div_10() {
        let fields = vec![
            DynamicSummaryQueryField::CpuUsage,
            DynamicSummaryQueryField::UsedMemory,
            DynamicSummaryQueryField::LoadOne,
        ];

        let stmt = build_single_last_select(1i16, &fields, DatabaseBackend::Postgres);
        let sql = StatementBuilder::build(&stmt, &DatabaseBackend::Postgres).to_string();

        assert!(
            sql.contains(r#""cpu_usage" / 10 AS "cpu_usage""#),
            "PostgreSQL SQL should descale cpu_usage in SQL: {sql}"
        );
        assert!(
            sql.contains(r#""load_one" / 10 AS "load_one""#),
            "PostgreSQL SQL should descale load_one in SQL: {sql}"
        );
        assert!(
            !sql.contains(r#""used_memory" / 10"#),
            "PostgreSQL SQL should NOT descale used_memory in SQL: {sql}"
        );
    }

    #[test]
    fn test_sqlite_sql_has_no_div_10() {
        let fields = vec![
            DynamicSummaryQueryField::CpuUsage,
            DynamicSummaryQueryField::UsedMemory,
            DynamicSummaryQueryField::LoadOne,
        ];

        let stmt = build_single_last_select(1i16, &fields, DatabaseBackend::Sqlite);
        let sql = StatementBuilder::build(&stmt, &DatabaseBackend::Sqlite).to_string();

        assert!(
            !sql.contains("/ 10"),
            "SQLite SQL should NOT contain / 10 expressions: {sql}"
        );
    }
}
