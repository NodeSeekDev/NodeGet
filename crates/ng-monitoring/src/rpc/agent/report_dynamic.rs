use crate::data_structure::DynamicMonitoringData;
use crate::monitoring_buffer;
use crate::monitoring_last_cache::MonitoringLastCache;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::dynamic_monitoring;
use ng_token::get::check_token_limit;
use sea_orm::{ActiveValue, Set};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn report_dynamic(
    token: String,
    dynamic_monitoring_data: DynamicMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let agent_uuid = dynamic_monitoring_data.uuid;
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

        let uuid_id = MonitoringUuidCache::global()
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let timestamp = dynamic_monitoring_data.time.cast_signed();

        let cpu_val = serde_json::to_value(&dynamic_monitoring_data.cpu)
            .map_err(|e| NodegetError::SerializationError(format!("cpu_data: {e}")))?;
        let ram_val = serde_json::to_value(&dynamic_monitoring_data.ram)
            .map_err(|e| NodegetError::SerializationError(format!("ram_data: {e}")))?;
        let load_val = serde_json::to_value(&dynamic_monitoring_data.load)
            .map_err(|e| NodegetError::SerializationError(format!("load_data: {e}")))?;
        let system_val = serde_json::to_value(&dynamic_monitoring_data.system)
            .map_err(|e| NodegetError::SerializationError(format!("system_data: {e}")))?;
        let disk_val = serde_json::to_value(&dynamic_monitoring_data.disk)
            .map_err(|e| NodegetError::SerializationError(format!("disk_data: {e}")))?;
        let network_val = serde_json::to_value(&dynamic_monitoring_data.network)
            .map_err(|e| NodegetError::SerializationError(format!("network_data: {e}")))?;
        let gpu_val = serde_json::to_value(&dynamic_monitoring_data.gpu)
            .map_err(|e| NodegetError::SerializationError(format!("gpu_data: {e}")))?;

        let in_data = dynamic_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(timestamp),
            storage_time: Set(Some(get_local_timestamp_ms_i64()?)),
            cpu_data: Set(cpu_val.clone()),
            ram_data: Set(ram_val.clone()),
            load_data: Set(load_val.clone()),
            system_data: Set(system_val.clone()),
            disk_data: Set(disk_val.clone()),
            network_data: Set(network_val.clone()),
            gpu_data: Set(gpu_val.clone()),
        };

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Received dynamic data, sending to buffer");

        monitoring_buffer::get().dynamic_mon.send(in_data);

        let mut obj = serde_json::Map::with_capacity(9);
        obj.insert(
            "uuid".to_owned(),
            serde_json::Value::String(agent_uuid.to_string()),
        );
        obj.insert(
            "timestamp".to_owned(),
            serde_json::Value::Number(timestamp.into()),
        );
        obj.insert("cpu".to_owned(), cpu_val);
        obj.insert("ram".to_owned(), ram_val);
        obj.insert("load".to_owned(), load_val);
        obj.insert("system".to_owned(), system_val);
        obj.insert("disk".to_owned(), disk_val);
        obj.insert("network".to_owned(), network_val);
        obj.insert("gpu".to_owned(), gpu_val);
        MonitoringLastCache::global()
            .update_dynamic_prebuilt(agent_uuid, serde_json::Value::Object(obj));

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Dynamic data buffered successfully");

        RawValue::from_string(r#"{"status":"buffered"}"#.to_owned())
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
