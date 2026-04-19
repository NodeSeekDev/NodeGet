use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::{DynamicSummaryAvgQuery, DynamicSummaryQueryField};
use nodeget_lib::permission::data_structure::{
    DynamicMonitoringSummary, Permission, Scope,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde_json::Value;
use serde_json::value::RawValue;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, warn};

static ERROR_COUNTER: AtomicU64 = AtomicU64::new(0);
fn generate_error_id() -> u64 {
    ERROR_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, FromQueryResult)]
struct JsonAggRow {
    data: Value,
}

pub async fn query_dynamic_summary_avg(
    token: String,
    query: DynamicSummaryAvgQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(
            target: "monitoring",
            uuid = %query.uuid,
            fields_count = query.fields.len(),
            points = query.points,
            timestamp_from = ?query.timestamp_from,
            timestamp_to = ?query.timestamp_to,
            "Dynamic summary avg query requested"
        );

        validate_avg_query(&query)?;

        debug!(target: "monitoring", "Dynamic summary avg query validation passed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(query.uuid)],
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

        debug!(target: "monitoring", uuid = %query.uuid, "Dynamic summary avg query permission check passed");

        let uuid_cache = MonitoringUuidCache::global();
        let uuid_id = uuid_cache.get_id(&query.uuid).await.ok_or_else(|| {
            NodegetError::NotFound(format!("Agent UUID {} not found in monitoring_uuid table", query.uuid))
        })?;

        let db = AgentRpcImpl::get_db()?;
        ensure_postgres_backend(db).map_err(|e| {
            error!(target: "monitoring", error = %e, "Dynamic summary avg query requires PostgreSQL backend");
            e
        })?;
        debug!(target: "monitoring", uuid = %query.uuid, uuid_id, "Executing dynamic summary avg SQL query");
        query_summary_avg_postgres(db, &query, uuid_id).await
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let raw = anyhow_error_to_raw(&e).unwrap_or_else(|_| {
                RawValue::from_string(
                    r#"{"error_id":999,"error_message":"Internal error"}"#.to_owned(),
                )
                .unwrap_or_else(|_| RawValue::from_string("null".to_owned()).unwrap())
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

fn validate_avg_query(query: &DynamicSummaryAvgQuery) -> anyhow::Result<()> {
    if query.fields.is_empty() {
        warn!(target: "monitoring", "Dynamic summary avg query validation failed: fields empty");
        return Err(NodegetError::InvalidInput(
            "fields cannot be empty for dynamic_summary_avg_query".to_owned(),
        )
        .into());
    }
    if query.points == 0 {
        warn!(target: "monitoring", "Dynamic summary avg query validation failed: points is 0");
        return Err(NodegetError::InvalidInput("points must be >= 1".to_owned()).into());
    }
    if let (Some(start), Some(end)) = (query.timestamp_from, query.timestamp_to)
        && start > end
    {
        warn!(target: "monitoring", start, end, "Dynamic summary avg query validation failed: timestamp_from > timestamp_to");
        return Err(NodegetError::InvalidInput(
            "timestamp_from cannot be greater than timestamp_to".to_owned(),
        )
        .into());
    }
    Ok(())
}

fn ensure_postgres_backend(db: &DatabaseConnection) -> anyhow::Result<()> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return Ok(());
    }

    Err(NodegetError::InvalidInput(
        "agent_query_dynamic_summary_avg currently only supports PostgreSQL"
            .to_owned(),
    )
    .into())
}

async fn query_summary_avg_postgres(
    db: &DatabaseConnection,
    query: &DynamicSummaryAvgQuery,
    uuid_id: i16,
) -> anyhow::Result<Box<RawValue>> {
    let sql = build_postgres_summary_avg_sql(&query.fields);
    tracing::trace!(target: "monitoring", fields_count = query.fields.len(), "Dynamic summary avg SQL generated");
    let statement = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [
            query.uuid.to_string().into(),
            query.timestamp_from.into(),
            query.timestamp_to.into(),
            i64::try_from(query.points)
                .map_err(|_| NodegetError::InvalidInput("points is too large".to_owned()))?
                .into(),
            (uuid_id as i32).into(),
        ],
    );

    let row = JsonAggRow::find_by_statement(statement)
        .one(db)
        .await
        .map_err(|e| {
            let error_id = generate_error_id();
            tracing::error!(target: "monitoring", error_id = error_id, error = %e, "Failed to query dynamic summary avg in postgres");
            NodegetError::DatabaseError(format!("Database error occurred. Reference: {error_id}"))
        })?;

    let json = row.map_or(Value::Array(Vec::new()), |r| r.data);
    let result_count = if let Value::Array(ref arr) = json { arr.len() } else { 1 };
    let json = serde_json::to_string(&json)
        .map_err(|e| NodegetError::SerializationError(format!("Serialization failed: {e}")))?;

    debug!(target: "monitoring", result_count, "Dynamic summary avg query completed");

    RawValue::from_string(json).map_err(|e| {
        NodegetError::SerializationError(format!("RawValue creation error: {e}")).into()
    })
}

fn build_postgres_summary_avg_sql(fields: &[DynamicSummaryQueryField]) -> String {
    // Select columns for the CTE
    let select_columns = fields.iter().fold(String::new(), |mut output, field| {
        write!(output, ", {}", field.column_name()).expect("writing to String should not fail");
        output
    });

    // Aggregate columns: simple AVG for flat columns, with /10.0 descaling for scaled fields
    let aggregate_columns = fields
        .iter()
        .map(|field| {
            if field.is_scaled() {
                format!(
                    "AVG({col})::double precision / 10.0 AS {key}",
                    col = field.column_name(),
                    key = field.json_key()
                )
            } else {
                format!(
                    "AVG({col})::double precision AS {key}",
                    col = field.column_name(),
                    key = field.json_key()
                )
            }
        })
        .collect::<Vec<_>>()
        .join(",\n            ");

    let aggregate_columns = if aggregate_columns.is_empty() {
        String::new()
    } else {
        format!(",\n            {aggregate_columns}")
    };

    // Final JSON fields
    let final_json_fields = fields.iter().fold(String::new(), |mut output, field| {
        write!(output, ", '{}', agg.{}", field.json_key(), field.json_key())
            .expect("writing to String should not fail");
        output
    });

    format!(
        r"
WITH filtered AS MATERIALIZED (
    SELECT 
        timestamp{select_columns},
        MIN(timestamp) OVER () AS min_ts,
        MAX(timestamp) OVER () AS max_ts
    FROM dynamic_monitoring_summary
    WHERE uuid_id = $5::smallint
      AND ($2::bigint IS NULL OR timestamp >= $2)
      AND ($3::bigint IS NULL OR timestamp <= $3)
),
bucketed AS (
    SELECT
        CASE
            WHEN min_ts IS NULL THEN NULL
            WHEN min_ts = max_ts OR $4::bigint <= 1 THEN 0
            ELSE LEAST(
                $4::bigint - 1,
                ((timestamp - min_ts) * $4::bigint) / NULLIF(max_ts - min_ts, 0)
            )
        END AS bucket,
        timestamp{select_columns}
    FROM filtered
),
agg AS (
    SELECT
        bucket AS bucket,
        AVG(timestamp)::bigint AS timestamp{aggregate_columns}
    FROM bucketed
    WHERE bucket IS NOT NULL
    GROUP BY bucket
    ORDER BY bucket
)
SELECT COALESCE(
    jsonb_agg(
        jsonb_build_object(
            'uuid', $1::text,
            'timestamp', agg.timestamp{final_json_fields}
        )
        ORDER BY agg.bucket
    ),
    '[]'::jsonb
) AS data
FROM agg
"
    )
}
