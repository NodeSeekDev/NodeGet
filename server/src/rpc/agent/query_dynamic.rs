use crate::entity::dynamic_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::{
    DynamicDataQuery, DynamicDataQueryField, QueryCondition,
};
use nodeget_lib::permission::data_structure::{
    DynamicMonitoring, Permission, Scope,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use nodeget_lib::utils::server_json::rename_and_fix_json;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder,
    QuerySelect, SelectModel, Selector,
};
use serde_json::value::RawValue;

pub async fn query_dynamic(
    token: String,
    dynamic_data_query: DynamicDataQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
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

        let permissions: Vec<Permission> = dynamic_data_query
            .fields
            .iter()
            .map(|field| Permission::DynamicMonitoring(DynamicMonitoring::Read(*field)))
            .collect();

        let is_allowed = check_token_limit(&token_or_auth, scopes, permissions).await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient DynamicMonitoring Read permissions".to_string(),
            )
            .into());
        }

        let db = AgentRpcImpl::get_db()?;

        let query = dynamic_monitoring::Entity::find()
            .select_only()
            .column(dynamic_monitoring::Column::Uuid)
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

        let query = dynamic_data_query
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(uuid) => q.filter(dynamic_monitoring::Column::Uuid.eq(uuid)),
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
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = limit_count {
            query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query.order_by(dynamic_monitoring::Column::Timestamp, Order::Asc)
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
            limit_count.unwrap_or(5000),
        )
        .await
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let raw = anyhow_error_to_raw(&e).unwrap_or_else(|_| {
                RawValue::from_string(r#"{"error_id":999,"error_message":"Internal error"}"#.to_string())
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
) -> anyhow::Result<Box<RawValue>> {
    let mut stream = query.stream(db).await.map_err(|e| {
        error!("Database query error: {e}");
        NodegetError::DatabaseError(format!("Database query error: {e}"))
    })?;

    let capacity = capacity_hint as usize * 200;
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut v) => {
                if let Some(obj) = v.as_object_mut() {
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
                    error!("Serialization failed: {e}");
                    return Err(NodegetError::SerializationError(format!("Serialization failed: {e}")).into());
                }
            }
            Err(e) => {
                error!("Stream read error: {e}");
                return Err(NodegetError::DatabaseError(format!("Stream read error: {e}")).into());
            }
        }
    }

    output_buffer.push(b']');

    let json_string = String::from_utf8(output_buffer).map_err(|e| {
        error!("UTF8 conversion error: {e}");
        NodegetError::SerializationError("UTF8 conversion error (internal)".to_string())
    })?;

    let raw_value = RawValue::from_string(json_string).map_err(|e| {
        error!("RawValue creation error: {e}");
        NodegetError::SerializationError("RawValue creation error".to_string())
    })?;

    Ok(raw_value)
}
