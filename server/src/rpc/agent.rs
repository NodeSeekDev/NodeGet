use crate::DB;
use crate::entity::{dynamic_monitoring, static_monitoring};
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use nodeget_lib::utils::error_message::generate_error_message;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait, Set};
use serde::Serialize;

#[rpc(server, namespace = "agent")]
pub trait Rpc {
    #[method(name = "report_static")]
    async fn report_static(&self, token: String, data: serde_json::Value) -> serde_json::Value;

    #[method(name = "report_dynamic")]
    async fn report_dynamic(&self, token: String, data: serde_json::Value) -> serde_json::Value;
}
pub struct AgentRpcImpl;

impl AgentRpcImpl {
    fn try_set_json<T: Serialize>(
        val: T,
    ) -> Result<sea_orm::ActiveValue<serde_json::Value>, String> {
        serde_json::to_value(val)
            .map(Set)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    fn get_db() -> Result<&'static DatabaseConnection, (i64, String)> {
        DB.get()
            .ok_or_else(|| (102, "DB not initialized".to_string()))
    }
}

#[async_trait]
impl RpcServer for AgentRpcImpl {
    async fn report_static(&self, _token: String, data: serde_json::Value) -> serde_json::Value {
        let process_logic = async {
            let db = Self::get_db()?;

            let parsed: StaticMonitoringData = serde_json::from_value(data)
                .map_err(|e| (101, format!("Unable to parse json data: {e}")))?;

            let in_data = static_monitoring::ActiveModel {
                id: ActiveValue::default(),
                uuid: Set(parsed.uuid),
                timestamp: Set(parsed.time.cast_signed()),

                cpu_data: Self::try_set_json(parsed.cpu).map_err(|e| (101, e))?,
                system_data: Self::try_set_json(parsed.system).map_err(|e| (101, e))?,
                gpu_data: Self::try_set_json(parsed.gpu).map_err(|e| (101, e))?,
            };

            let result = static_monitoring::Entity::insert(in_data)
                .exec(db)
                .await
                .map_err(|e| (103, format!("Database insert error: {e}")))?;

            Ok(result.last_insert_id)
        };

        match process_logic.await {
            Ok(new_id) => serde_json::json!({ "id": new_id }),
            Err((code, msg)) => generate_error_message(code, msg),
        }
    }

    async fn report_dynamic(&self, _token: String, data: serde_json::Value) -> serde_json::Value {
        let process_logic = async {
            let db = Self::get_db()?;

            let parsed: DynamicMonitoringData = serde_json::from_value(data)
                .map_err(|e| (101, format!("Unable to parse json data: {e}")))?;

            let in_data = dynamic_monitoring::ActiveModel {
                id: ActiveValue::default(),
                uuid: Set(parsed.uuid),
                timestamp: Set(parsed.time.cast_signed()),

                cpu_data: Self::try_set_json(parsed.cpu).map_err(|e| (101, e))?,
                ram_data: Self::try_set_json(parsed.ram).map_err(|e| (101, e))?,
                load_data: Self::try_set_json(parsed.load).map_err(|e| (101, e))?,
                system_data: Self::try_set_json(parsed.system).map_err(|e| (101, e))?,
                disk_data: Self::try_set_json(parsed.disk).map_err(|e| (101, e))?,
                network_data: Self::try_set_json(parsed.network).map_err(|e| (101, e))?,
                gpu_data: Self::try_set_json(parsed.gpu).map_err(|e| (101, e))?,
            };

            let result = dynamic_monitoring::Entity::insert(in_data)
                .exec(db)
                .await
                .map_err(|e| (103, format!("Database insert error: {e}")))?;

            Ok(result.last_insert_id)
        };

        match process_logic.await {
            Ok(new_id) => serde_json::json!({ "id": new_id }),
            Err((code, msg)) => generate_error_message(code, msg),
        }
    }
}
