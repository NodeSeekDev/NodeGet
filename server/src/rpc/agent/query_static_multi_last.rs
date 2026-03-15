use crate::entity::static_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::StaticDataQueryField;
use nodeget_lib::permission::data_structure::{Permission, Scope, StaticMonitoring};
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
use uuid::Uuid;

pub async fn static_data_multi_last_query(
    token: String,
    uuids: Vec<Uuid>,
    fields: Vec<StaticDataQueryField>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
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
                if check_token_limit(&token_or_auth, scopes.clone(), vec![permission]).await? {
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
            check_token_limit(&token_or_auth, scopes, permissions).await?
        };

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient StaticMonitoring Read permissions".to_owned(),
            )
            .into());
        }

        let db = AgentRpcImpl::get_db()?;
        let statement = build_union_last_statement(&deduped_uuids, &fields, db)?;

        let field_mappings: Vec<(&str, &str)> = fields
            .iter()
            .map(|field| (field.column_name(), field.json_key()))
            .collect();

        execute_statement_query(db, statement, &field_mappings, deduped_uuids.len()).await
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
    uuids: &[Uuid],
    fields: &[StaticDataQueryField],
    db: &DatabaseConnection,
) -> anyhow::Result<Statement> {
    let mut uuid_iter = uuids.iter().copied();
    let first_uuid = uuid_iter
        .next()
        .ok_or_else(|| NodegetError::InvalidInput("The uuids list cannot be empty".to_owned()))?;

    let mut union_query = build_single_last_select(first_uuid, fields);
    for uuid in uuid_iter {
        union_query.union(UnionType::All, build_single_last_select(uuid, fields));
    }

    Ok(StatementBuilder::build(
        &union_query,
        &db.get_database_backend(),
    ))
}

fn build_single_last_select(uuid: Uuid, fields: &[StaticDataQueryField]) -> SelectStatement {
    let inner_query = static_monitoring::Entity::find()
        .select_only()
        .column(static_monitoring::Column::Uuid)
        .column(static_monitoring::Column::Timestamp);

    let inner_query = fields.iter().fold(inner_query, |query, field| match field {
        StaticDataQueryField::Cpu => query.column(static_monitoring::Column::CpuData),
        StaticDataQueryField::System => query.column(static_monitoring::Column::SystemData),
        StaticDataQueryField::Gpu => query.column(static_monitoring::Column::GpuData),
    });

    let inner_query = inner_query
        .filter(static_monitoring::Column::Uuid.eq(uuid))
        .order_by(static_monitoring::Column::Timestamp, Order::Desc)
        .limit(1)
        .into_query();

    let alias = Alias::new("last_row");
    let mut wrapped = Query::select();
    wrapped
        .column((alias.clone(), Alias::new("uuid")))
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

async fn execute_statement_query(
    db: &DatabaseConnection,
    statement: Statement,
    field_mappings: &[(&str, &str)],
    capacity_hint: usize,
) -> anyhow::Result<Box<RawValue>> {
    let mut stream = serde_json::Value::find_by_statement(statement)
        .stream(db)
        .await
        .map_err(|e| {
            error!("Database query error: {e}");
            NodegetError::DatabaseError(format!("Database query error: {e}"))
        })?;

    let capacity = capacity_hint.saturating_mul(200);
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut value) => {
                if let Some(obj) = value.as_object_mut() {
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
                    error!("Serialization failed: {e}");
                    return Err(NodegetError::SerializationError(format!(
                        "Serialization failed: {e}"
                    ))
                    .into());
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
