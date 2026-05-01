use crate::entity::dynamic_monitoring_summary;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::data_structure::DynamicMonitoringSummaryData;
use nodeget_lib::permission::data_structure::{DynamicMonitoringSummary, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ActiveValue, Set};
use serde_json::value::RawValue;
use std::str::FromStr;
use tracing::debug;

pub async fn report_dynamic_summary(
    token: String,
    data: DynamicMonitoringSummaryData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let agent_uuid = uuid::Uuid::from_str(&data.uuid)
            .map_err(|e| NodegetError::ParseError(format!("Invalid UUID format: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Write,
            )],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Write permission for this Agent"
                    .to_owned(),
            )
                .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: permission check passed");

        let uuid_id = crate::monitoring_uuid_cache::MonitoringUuidCache::global()
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let in_data = dynamic_monitoring_summary::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(data.time.cast_signed()),
            cpu_usage: Set(data.cpu_usage),
            gpu_usage: Set(data.gpu_usage),
            used_swap: Set(data.used_swap),
            total_swap: Set(data.total_swap),
            used_memory: Set(data.used_memory),
            total_memory: Set(data.total_memory),
            available_memory: Set(data.available_memory),
            load_one: Set(data.load_one),
            load_five: Set(data.load_five),
            load_fifteen: Set(data.load_fifteen),
            uptime: Set(data.uptime),
            boot_time: Set(data.boot_time),
            process_count: Set(data.process_count),
            total_space: Set(data.total_space),
            available_space: Set(data.available_space),
            read_speed: Set(data.read_speed),
            write_speed: Set(data.write_speed),
            tcp_connections: Set(data.tcp_connections),
            udp_connections: Set(data.udp_connections),
            total_received: Set(data.total_received),
            total_transmitted: Set(data.total_transmitted),
            transmit_speed: Set(data.transmit_speed),
            receive_speed: Set(data.receive_speed),
        };

        debug!(target: "monitoring", agent_uuid = %data.uuid, "Received dynamic summary data, sending to buffer");

        crate::monitoring_buffer::get()
            .dynamic_summary
            .send(in_data);

        // Update in-memory last-cache (used by multi-last queries, zero DB hit)
        crate::monitoring_last_cache::MonitoringLastCache::global()
            .update_dynamic_summary(agent_uuid, data.time.cast_signed(), &data)
            .await;

        debug!(target: "monitoring", agent_uuid = %data.uuid, "Dynamic summary data buffered successfully");

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
