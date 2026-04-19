use crate::entity::dynamic_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::monitoring::data_structure::DynamicMonitoringData;
use nodeget_lib::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ActiveValue, Set};
use serde_json::value::RawValue;
use std::str::FromStr;
use tracing::debug;

pub async fn report_dynamic(
    token: String,
    dynamic_monitoring_data: DynamicMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let agent_uuid = uuid::Uuid::from_str(&dynamic_monitoring_data.uuid)
            .map_err(|e| NodegetError::ParseError(format!("Invalid UUID format: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::DynamicMonitoring(DynamicMonitoring::Write)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Write permission for this Agent"
                    .to_owned(),
            )
            .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: permission check passed");

        let uuid_id = crate::monitoring_uuid_cache::MonitoringUuidCache::global()
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let in_data = dynamic_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(dynamic_monitoring_data.time.cast_signed()),
            cpu_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.cpu)
                .map_err(|e| NodegetError::SerializationError(format!("cpu_data: {e}")))?,
            ram_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.ram)
                .map_err(|e| NodegetError::SerializationError(format!("ram_data: {e}")))?,
            load_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.load)
                .map_err(|e| NodegetError::SerializationError(format!("load_data: {e}")))?,
            system_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.system)
                .map_err(|e| NodegetError::SerializationError(format!("system_data: {e}")))?,
            disk_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.disk)
                .map_err(|e| NodegetError::SerializationError(format!("disk_data: {e}")))?,
            network_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.network)
                .map_err(|e| NodegetError::SerializationError(format!("network_data: {e}")))?,
            gpu_data: AgentRpcImpl::try_set_json(dynamic_monitoring_data.gpu)
                .map_err(|e| NodegetError::SerializationError(format!("gpu_data: {e}")))?,
        };

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Received dynamic data, sending to buffer");

        crate::monitoring_buffer::get()
            .dynamic_mon
            .send(in_data)
            .map_err(|_| NodegetError::DatabaseError("Buffer closed".to_owned()))?;

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Dynamic data buffered successfully");

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
