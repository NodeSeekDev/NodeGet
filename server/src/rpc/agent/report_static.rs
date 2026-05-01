use crate::entity::static_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::data_structure::StaticMonitoringData;
use nodeget_lib::permission::data_structure::{Permission, Scope, StaticMonitoring};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use std::str::FromStr;
use tracing::{debug, error};

pub async fn report_static(
    token: String,
    static_monitoring_data: StaticMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let agent_uuid = uuid::Uuid::from_str(&static_monitoring_data.uuid)
            .map_err(|e| NodegetError::ParseError(format!("Invalid UUID format: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::StaticMonitoring(StaticMonitoring::Write)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing StaticMonitoring Write permission for this Agent"
                    .to_string(),
            )
            .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: permission check passed");

        let uuid_id = crate::monitoring_uuid_cache::MonitoringUuidCache::global()
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        // Fast path: check in-memory hash cache first to avoid DB query
        let hash_cache = crate::static_hash_cache::StaticHashCache::global();
        if hash_cache
            .is_duplicate(uuid_id, &static_monitoring_data.data_hash)
            .await
        {
            debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data hash cached as duplicate, skipping");
            return RawValue::from_string(
                r#"{"status":"skipped","reason":"duplicate_hash"}"#.to_owned(),
            )
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        // Slow path: check DB for hash existence (covers hashes from before cache was populated)
        let db = <AgentRpcImpl as crate::rpc::RpcHelper>::get_db()?;
        let exists = static_monitoring::Entity::find()
            .filter(static_monitoring::Column::UuidId.eq(uuid_id))
            .filter(static_monitoring::Column::DataHash.eq(static_monitoring_data.data_hash.as_slice()))
            .one(db)
            .await
            .map_err(|e| {
                error!(target: "monitoring", agent_uuid = %agent_uuid, error = %e, "report_static: DB hash check failed");
                NodegetError::DatabaseError(e.to_string())
            })?;

        if exists.is_some() {
            // Update cache so next time we skip the DB query
            hash_cache
                .update(uuid_id, static_monitoring_data.data_hash.clone())
                .await;
            // Also update last-cache so subsequent multi-last queries can hit the cache
            // even when the data hash is unchanged (common for static monitoring).
            crate::monitoring_last_cache::MonitoringLastCache::global()
                .update_static(agent_uuid, static_monitoring_data.time.cast_signed(), &static_monitoring_data)
                .await;
            debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data hash already exists, skipping");
            return RawValue::from_string(
                r#"{"status":"skipped","reason":"duplicate_hash"}"#.to_owned(),
            )
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        // Update in-memory last-cache (used by multi-last queries, zero DB hit)
        crate::monitoring_last_cache::MonitoringLastCache::global()
            .update_static(agent_uuid, static_monitoring_data.time.cast_signed(), &static_monitoring_data)
            .await;

        let data_hash = static_monitoring_data.data_hash;
        let in_data = static_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(static_monitoring_data.time.cast_signed()),
            cpu_data: AgentRpcImpl::try_set_json(static_monitoring_data.cpu)
                .map_err(|e| NodegetError::SerializationError(format!("cpu_data: {e}")))?,
            system_data: AgentRpcImpl::try_set_json(static_monitoring_data.system)
                .map_err(|e| NodegetError::SerializationError(format!("system_data: {e}")))?,
            gpu_data: AgentRpcImpl::try_set_json(static_monitoring_data.gpu)
                .map_err(|e| NodegetError::SerializationError(format!("gpu_data: {e}")))?,
            data_hash: Set(data_hash.clone()),
        };

        debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Received static data, sending to buffer");

        crate::monitoring_buffer::get().static_mon.send(in_data);

        // Update hash cache after successful buffer
        hash_cache.update(uuid_id, data_hash).await;

        debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data buffered successfully");

        RawValue::from_string(r#"{"status":"buffered"}"#.to_owned())
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
