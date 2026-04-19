use crate::entity::dynamic_monitoring_summary;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::{DynamicSummaryQuery, DynamicSummaryQueryField, QueryCondition};
use nodeget_lib::permission::data_structure::{
    DynamicMonitoringSummary, Permission, Scope,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use sea_orm::{
    ColumnTrait, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder, QuerySelect, SelectModel,
    Selector,
};
use sea_orm::prelude::Expr;
use serde_json::value::RawValue;
use serde_json::Value;
use tracing::{debug, error};

pub async fn query_dynamic_summary(
    token: String,
    query_data: DynamicSummaryQuery,
) -> RpcResult<Box<RawValue>> {
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
            scopes,
            vec![Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Read,
            )],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Read permission".to_string(),
            )
            .into());
        }

        debug!(target: "monitoring", conditions_count = query_data.condition.len(), fields_count = query_data.fields.len(), "Dynamic summary query permission check passed");

        let db = AgentRpcImpl::get_db()?;

        let uuid_cache = MonitoringUuidCache::global();

        let query = dynamic_monitoring_summary::Entity::find()
            .select_only()
            .column(dynamic_monitoring_summary::Column::UuidId)
            .column(dynamic_monitoring_summary::Column::Timestamp);

        let query = if query_data.fields.is_empty() {
            // Select all data columns (scaled fields use /10.0)
            query
                .column_as(Expr::col(dynamic_monitoring_summary::Column::CpuUsage).div(10.0), "cpu_usage")
                .column(dynamic_monitoring_summary::Column::GpuUsage)
                .column(dynamic_monitoring_summary::Column::UsedSwap)
                .column(dynamic_monitoring_summary::Column::TotalSwap)
                .column(dynamic_monitoring_summary::Column::UsedMemory)
                .column(dynamic_monitoring_summary::Column::TotalMemory)
                .column(dynamic_monitoring_summary::Column::AvailableMemory)
                .column_as(Expr::col(dynamic_monitoring_summary::Column::LoadOne).div(10.0), "load_one")
                .column_as(Expr::col(dynamic_monitoring_summary::Column::LoadFive).div(10.0), "load_five")
                .column_as(Expr::col(dynamic_monitoring_summary::Column::LoadFifteen).div(10.0), "load_fifteen")
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
            query_data.fields.iter().fold(query, |q, field| {
                select_field(q, field)
            })
        };

        let mut limit_count = None;
        let mut is_last = false;

        // Pre-resolve UUID conditions to uuid_id via cache
        let mut uuid_ids: Vec<i16> = Vec::new();
        for cond in &query_data.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                let uuid_id = uuid_cache.get_id(uuid).await.ok_or_else(|| {
                    anyhow::anyhow!(NodegetError::NotFound(format!("Unknown agent UUID: {uuid}")))
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
                QueryCondition::Limit(n) => {
                    limit_count = Some(n);
                    q
                }
                QueryCondition::Last => {
                    is_last = true;
                    q
                }
            });

        let query = if is_last {
            query
                .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = limit_count {
            query
                .order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query.order_by(dynamic_monitoring_summary::Column::Timestamp, Order::Asc)
        };

        execute_query(db, query.into_json(), limit_count.unwrap_or(5000), uuid_cache).await
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

/// Select a field into the query, applying /10.0 descaling for scaled fields.
fn select_field<E>(
    q: sea_orm::Select<E>,
    field: &DynamicSummaryQueryField,
) -> sea_orm::Select<E>
where
    E: EntityTrait,
{
    let col = field_to_column(field);
    if field.is_scaled() {
        q.column_as(Expr::col(col).div(10.0), field.column_name())
    } else {
        q.column(col)
    }
}

pub(crate) fn field_to_column(
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

    let capacity = capacity_hint as usize * 200;
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;
    let mut result_count: usize = 0;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut v) => {
                result_count += 1;

                // Translate uuid_id (i16) → uuid (string) for API compatibility
                if let Value::Object(ref mut map) = v {
                    if let Some(Value::Number(n)) = map.remove("uuid_id") {
                        if let Some(id) = n.as_i64() {
                            if let Some(uuid) = uuid_cache.get_uuid(id as i16).await {
                                map.insert("uuid".to_string(), Value::String(uuid.to_string()));
                            }
                        }
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

    debug!(target: "monitoring", result_count = result_count, "Dynamic monitoring summary query completed");

    Ok(raw_value)
}
