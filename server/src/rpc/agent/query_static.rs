use crate::entity::static_monitoring;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::{QueryCondition, StaticDataQuery, StaticDataQueryField};
use nodeget_lib::permission::data_structure::{Permission, Scope, StaticMonitoring};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use nodeget_lib::utils::server_json::rename_and_fix_json;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder,
    QuerySelect, SelectModel, Selector,
};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, error};

pub async fn query_static(
    token: String,
    static_data_query: StaticDataQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "monitoring", conditions_count = static_data_query.condition.len(), fields_count = static_data_query.fields.len(), "Static query request received");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;

        for cond in &static_data_query.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }

        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let is_allowed = if static_data_query.fields.is_empty() {
            let mut any_allowed = false;
            for permission in [
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::Cpu)),
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::System)),
                Permission::StaticMonitoring(StaticMonitoring::Read(StaticDataQueryField::Gpu)),
            ] {
                if check_token_limit(&token_or_auth, scopes.clone(), vec![permission]).await? {
                    any_allowed = true;
                    break;
                }
            }
            any_allowed
        } else {
            let permissions: Vec<Permission> = static_data_query
                .fields
                .iter()
                .map(|field| Permission::StaticMonitoring(StaticMonitoring::Read(*field)))
                .collect();

            check_token_limit(&token_or_auth, scopes, permissions).await?
        };

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient StaticMonitoring Read permissions".to_owned(),
            )
            .into());
        }

        debug!(target: "monitoring", conditions_count = static_data_query.condition.len(), fields_count = static_data_query.fields.len(), "Static query permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let uuid_cache = MonitoringUuidCache::global();

        let query = static_monitoring::Entity::find()
            .select_only()
            .column(static_monitoring::Column::UuidId)
            .column(static_monitoring::Column::Timestamp);

        let query = static_data_query
            .fields
            .iter()
            .fold(query, |q, field| match field {
                StaticDataQueryField::Cpu => q.column(static_monitoring::Column::CpuData),
                StaticDataQueryField::System => q.column(static_monitoring::Column::SystemData),
                StaticDataQueryField::Gpu => q.column(static_monitoring::Column::GpuData),
            });

        let mut limit_count = None;
        let mut is_last = false;

        // Pre-resolve UUID conditions to uuid_id via cache
        let mut uuid_ids: Vec<i16> = Vec::new();
        for cond in &static_data_query.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                let uuid_id = uuid_cache.get_id(uuid).await.ok_or_else(|| {
                    anyhow::anyhow!(NodegetError::NotFound(format!("Unknown agent UUID: {uuid}")))
                })?;
                uuid_ids.push(uuid_id);
            }
        }
        let mut uuid_id_iter = uuid_ids.into_iter();

        let query = static_data_query
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(_) => {
                    let uuid_id = uuid_id_iter.next().unwrap();
                    q.filter(static_monitoring::Column::UuidId.eq(uuid_id))
                }
                QueryCondition::TimestampFromTo(start, end) => q.filter(
                    static_monitoring::Column::Timestamp
                        .gte(start)
                        .and(static_monitoring::Column::Timestamp.lte(end)),
                ),
                QueryCondition::TimestampFrom(start) => {
                    q.filter(static_monitoring::Column::Timestamp.gte(start))
                }
                QueryCondition::TimestampTo(end) => {
                    q.filter(static_monitoring::Column::Timestamp.lte(end))
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
                .order_by(static_monitoring::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = limit_count {
            query
                .order_by(static_monitoring::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query.order_by(static_monitoring::Column::Timestamp, Order::Asc)
        };

        let field_mappings: Vec<(&str, &str)> = static_data_query
            .fields
            .iter()
            .map(|f| (f.column_name(), f.json_key()))
            .collect();

        execute_query(
            db,
            query.into_json(),
            &field_mappings,
            limit_count.unwrap_or(100),
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

async fn execute_query(
    db: &DatabaseConnection,
    query: Selector<SelectModel<serde_json::Value>>,
    field_mappings: &[(&str, &str)],
    capacity_hint: u64,
    uuid_cache: &MonitoringUuidCache,
) -> anyhow::Result<Box<RawValue>> {
    debug!(target: "monitoring", "Starting static query DB stream");
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
                if let Some(obj) = v.as_object_mut() {
                    // Translate uuid_id (i16) → uuid (string) for API compatibility
                    if let Some(Value::Number(n)) = obj.remove("uuid_id") {
                        if let Some(id) = n.as_i64() {
                            if let Some(uuid) = uuid_cache.get_uuid(id as i16).await {
                                obj.insert("uuid".to_string(), Value::String(uuid.to_string()));
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

    debug!(target: "monitoring", result_count = result_count, "Static monitoring query completed");

    Ok(raw_value)
}
