use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::generate_avg_error_id;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::query::{DynamicDataAvgQuery, DynamicDataQueryField};
use nodeget_lib::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::anyhow_error_to_raw;
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde_json::Value;
use serde_json::value::RawValue;
use std::fmt::Write;
use tracing::{debug, error, warn};

#[derive(Debug, FromQueryResult)]
struct JsonAggRow {
    data: Value,
}

pub async fn query_dynamic_avg(
    token: String,
    dynamic_data_avg_query: DynamicDataAvgQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(
            target: "monitoring",
            uuid = %dynamic_data_avg_query.uuid,
            fields_count = dynamic_data_avg_query.fields.len(),
            points = dynamic_data_avg_query.points,
            timestamp_from = ?dynamic_data_avg_query.timestamp_from,
            timestamp_to = ?dynamic_data_avg_query.timestamp_to,
            "Dynamic avg query requested"
        );

        validate_avg_query(&dynamic_data_avg_query)?;

        debug!(target: "monitoring", "Dynamic avg query validation passed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let permissions: Vec<Permission> = dynamic_data_avg_query
            .fields
            .iter()
            .map(|field| Permission::DynamicMonitoring(DynamicMonitoring::Read(*field)))
            .collect();

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(dynamic_data_avg_query.uuid)],
            permissions,
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient DynamicMonitoring Read permissions".to_owned(),
            )
            .into());
        }

        debug!(target: "monitoring", uuid = %dynamic_data_avg_query.uuid, "Dynamic avg query permission check passed");

        let uuid_cache = MonitoringUuidCache::global();
        let uuid_id = uuid_cache.get_id(&dynamic_data_avg_query.uuid).await.ok_or_else(|| {
            NodegetError::NotFound(format!("Agent UUID {} not found in monitoring_uuid table", dynamic_data_avg_query.uuid))
        })?;

        let db = AgentRpcImpl::get_db()?;
        ensure_postgres_backend(db).map_err(|e| {
            error!(target: "monitoring", error = %e, "Dynamic avg query requires PostgreSQL backend");
            e
        })?;
        debug!(target: "monitoring", uuid = %dynamic_data_avg_query.uuid, uuid_id, "Executing dynamic avg SQL query");
        query_dynamic_avg_postgres(db, &dynamic_data_avg_query, uuid_id).await
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

fn validate_avg_query(query: &DynamicDataAvgQuery) -> anyhow::Result<()> {
    if query.fields.is_empty() {
        warn!(target: "monitoring", "Dynamic avg query validation failed: fields empty");
        return Err(NodegetError::InvalidInput(
            "fields cannot be empty for dynamic_data_avg_query".to_owned(),
        )
        .into());
    }
    if query.points == 0 {
        warn!(target: "monitoring", "Dynamic avg query validation failed: points is 0");
        return Err(NodegetError::InvalidInput("points must be >= 1".to_owned()).into());
    }
    if let (Some(start), Some(end)) = (query.timestamp_from, query.timestamp_to)
        && start > end
    {
        warn!(target: "monitoring", start, end, "Dynamic avg query validation failed: timestamp_from > timestamp_to");
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
        "agent_query_dynamic_avg currently only supports PostgreSQL; SQLite and other databases are disabled for this method"
            .to_owned(),
    )
        .into())
}

async fn query_dynamic_avg_postgres(
    db: &DatabaseConnection,
    query: &DynamicDataAvgQuery,
    uuid_id: i16,
) -> anyhow::Result<Box<RawValue>> {
    let sql = build_postgres_dynamic_avg_sql(&query.fields);
    tracing::trace!(target: "monitoring", fields_count = query.fields.len(), "Dynamic avg SQL generated");
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
            // 内部记录详细错误，向客户端返回通用错误
            let error_id = generate_avg_error_id();
            tracing::error!(target: "monitoring", error_id = error_id, error = %e, "Failed to query dynamic avg in postgres");
            NodegetError::DatabaseError(format!("Database error occurred. Reference: {error_id}"))
        })?;

    let json = row.map_or(Value::Array(Vec::new()), |r| r.data);
    let result_count = if let Value::Array(ref arr) = json { arr.len() } else { 1 };
    let json = serde_json::to_string(&json)
        .map_err(|e| NodegetError::SerializationError(format!("Serialization failed: {e}")))?;

    debug!(target: "monitoring", result_count, "Dynamic avg query completed");

    RawValue::from_string(json).map_err(|e| {
        NodegetError::SerializationError(format!("RawValue creation error: {e}")).into()
    })
}

fn build_postgres_dynamic_avg_sql(fields: &[DynamicDataQueryField]) -> String {
    let select_columns = fields.iter().fold(String::new(), |mut output, field| {
        write!(output, ", {}", field.column_name()).expect("writing to String should not fail");
        output
    });

    let aggregate_columns = fields
        .iter()
        .copied()
        .map(build_postgres_dynamic_field_aggregate_sql)
        .collect::<Vec<_>>()
        .join(",\n            ");

    let final_json_fields = fields.iter().fold(String::new(), |mut output, field| {
        write!(output, ", '{}', agg.{}", field.json_key(), field.json_key())
            .expect("writing to String should not fail");
        output
    });

    let aggregate_columns = if aggregate_columns.is_empty() {
        String::new()
    } else {
        format!(",\n            {aggregate_columns}")
    };

    format!(
        r"
WITH filtered AS MATERIALIZED (
    SELECT 
        timestamp{select_columns},
        MIN(timestamp) OVER () AS min_ts,
        MAX(timestamp) OVER () AS max_ts
    FROM dynamic_monitoring
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

fn build_postgres_dynamic_field_aggregate_sql(field: DynamicDataQueryField) -> String {
    match field {
        DynamicDataQueryField::Cpu => r"
jsonb_build_object(
    'per_core',
    (
        SELECT COALESCE(jsonb_agg(per_core.obj ORDER BY per_core.idx), '[]'::jsonb)
        FROM (
            SELECT
                arr.ord AS idx,
                jsonb_build_object(
                    'id', AVG(NULLIF(arr.elem->>'id', '')::numeric),
                    'cpu_usage', AVG(NULLIF(arr.elem->>'cpu_usage', '')::numeric),
                    'frequency_mhz', AVG(NULLIF(arr.elem->>'frequency_mhz', '')::numeric)
                ) AS obj
            FROM bucketed AS b2
            CROSS JOIN LATERAL jsonb_array_elements(COALESCE(b2.cpu_data->'per_core', '[]'::jsonb)) WITH ORDINALITY AS arr(elem, ord)
            WHERE b2.bucket = bucketed.bucket
            GROUP BY arr.ord
        ) AS per_core
    ),
    'total_cpu_usage', AVG(NULLIF(bucketed.cpu_data->>'total_cpu_usage', '')::numeric)
) AS cpu"
            .to_owned(),
        DynamicDataQueryField::Ram => r"
jsonb_build_object(
    'total_memory', AVG(NULLIF(bucketed.ram_data->>'total_memory', '')::numeric),
    'available_memory', AVG(NULLIF(bucketed.ram_data->>'available_memory', '')::numeric),
    'used_memory', AVG(NULLIF(bucketed.ram_data->>'used_memory', '')::numeric),
    'total_swap', AVG(NULLIF(bucketed.ram_data->>'total_swap', '')::numeric),
    'used_swap', AVG(NULLIF(bucketed.ram_data->>'used_swap', '')::numeric)
) AS ram"
            .to_owned(),
        DynamicDataQueryField::Load => r"
jsonb_build_object(
    'one', AVG(NULLIF(bucketed.load_data->>'one', '')::numeric),
    'five', AVG(NULLIF(bucketed.load_data->>'five', '')::numeric),
    'fifteen', AVG(NULLIF(bucketed.load_data->>'fifteen', '')::numeric)
) AS load"
            .to_owned(),
        DynamicDataQueryField::System => r"
jsonb_build_object(
    'process_count', AVG(NULLIF(bucketed.system_data->>'process_count', '')::numeric)
) AS system"
            .to_owned(),
        DynamicDataQueryField::Disk => r"
(
    SELECT COALESCE(jsonb_agg(disks.obj ORDER BY disks.idx), '[]'::jsonb)
    FROM (
        SELECT
            arr.ord AS idx,
            jsonb_build_object(
                'kind', NULL,
                'name', NULL,
                'file_system', NULL,
                'mount_point', NULL,
                'total_space', AVG(NULLIF(arr.elem->>'total_space', '')::numeric),
                'available_space', AVG(NULLIF(arr.elem->>'available_space', '')::numeric),
                'is_removable', NULL,
                'is_read_only', NULL,
                'read_speed', AVG(NULLIF(arr.elem->>'read_speed', '')::numeric),
                'write_speed', AVG(NULLIF(arr.elem->>'write_speed', '')::numeric)
            ) AS obj
        FROM bucketed AS b2
        CROSS JOIN LATERAL jsonb_array_elements(COALESCE(b2.disk_data, '[]'::jsonb)) WITH ORDINALITY AS arr(elem, ord)
        WHERE b2.bucket = bucketed.bucket
        GROUP BY arr.ord
    ) AS disks
) AS disk"
            .to_owned(),
        DynamicDataQueryField::Network => r"
jsonb_build_object(
    'interfaces',
    (
        SELECT COALESCE(jsonb_agg(interfaces.obj ORDER BY interfaces.idx), '[]'::jsonb)
        FROM (
            SELECT
                arr.ord AS idx,
                jsonb_build_object(
                    'interface_name', NULL,
                    'total_received', AVG(NULLIF(arr.elem->>'total_received', '')::numeric),
                    'total_transmitted', AVG(NULLIF(arr.elem->>'total_transmitted', '')::numeric),
                    'receive_speed', AVG(NULLIF(arr.elem->>'receive_speed', '')::numeric),
                    'transmit_speed', AVG(NULLIF(arr.elem->>'transmit_speed', '')::numeric)
                ) AS obj
            FROM bucketed AS b2
            CROSS JOIN LATERAL jsonb_array_elements(COALESCE(b2.network_data->'interfaces', '[]'::jsonb)) WITH ORDINALITY AS arr(elem, ord)
            WHERE b2.bucket = bucketed.bucket
            GROUP BY arr.ord
        ) AS interfaces
    ),
    'udp_connections', AVG(NULLIF(bucketed.network_data->>'udp_connections', '')::numeric),
    'tcp_connections', AVG(NULLIF(bucketed.network_data->>'tcp_connections', '')::numeric)
) AS network"
            .to_owned(),
        DynamicDataQueryField::Gpu => r"
(
    SELECT COALESCE(jsonb_agg(gpus.obj ORDER BY gpus.idx), '[]'::jsonb)
    FROM (
        SELECT
            arr.ord AS idx,
            jsonb_build_object(
                'id', AVG(NULLIF(arr.elem->>'id', '')::numeric),
                'used_memory', AVG(NULLIF(arr.elem->>'used_memory', '')::numeric),
                'total_memory', AVG(NULLIF(arr.elem->>'total_memory', '')::numeric),
                'graphics_clock_mhz', AVG(NULLIF(arr.elem->>'graphics_clock_mhz', '')::numeric),
                'sm_clock_mhz', AVG(NULLIF(arr.elem->>'sm_clock_mhz', '')::numeric),
                'memory_clock_mhz', AVG(NULLIF(arr.elem->>'memory_clock_mhz', '')::numeric),
                'video_clock_mhz', AVG(NULLIF(arr.elem->>'video_clock_mhz', '')::numeric),
                'utilization_gpu', AVG(NULLIF(arr.elem->>'utilization_gpu', '')::numeric),
                'utilization_memory', AVG(NULLIF(arr.elem->>'utilization_memory', '')::numeric),
                'temperature', AVG(NULLIF(arr.elem->>'temperature', '')::numeric)
            ) AS obj
        FROM bucketed AS b2
        CROSS JOIN LATERAL jsonb_array_elements(COALESCE(b2.gpu_data, '[]'::jsonb)) WITH ORDINALITY AS arr(elem, ord)
        WHERE b2.bucket = bucketed.bucket
        GROUP BY arr.ord
    ) AS gpus
) AS gpu"
            .to_owned(),
    }
}
