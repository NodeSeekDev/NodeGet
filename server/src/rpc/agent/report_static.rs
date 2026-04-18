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
use tracing::debug;

pub async fn report_static(
    token: String,
    static_monitoring_data: StaticMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let agent_uuid = uuid::Uuid::from_str(&static_monitoring_data.uuid)
            .map_err(|e| NodegetError::ParseError(format!("Invalid UUID format: {e}")))?;

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

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

        // 检查该 uuid + data_hash 是否已存在，若存在则跳过写入
        let db = <AgentRpcImpl as crate::rpc::RpcHelper>::get_db()?;
        let exists = static_monitoring::Entity::find()
            .filter(static_monitoring::Column::Uuid.eq(agent_uuid))
            .filter(static_monitoring::Column::DataHash.eq(static_monitoring_data.data_hash.as_slice()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        if exists.is_some() {
            debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data hash already exists, skipping");
            return RawValue::from_string(r#"{"status":"skipped","reason":"duplicate_hash"}"#.to_owned())
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        let data_hash = static_monitoring_data.data_hash;
        let in_data = static_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid: Set(agent_uuid),
            timestamp: Set(static_monitoring_data.time.cast_signed()),
            cpu_data: AgentRpcImpl::try_set_json(static_monitoring_data.cpu)
                .map_err(|e| NodegetError::SerializationError(format!("{e}")))?,
            system_data: AgentRpcImpl::try_set_json(static_monitoring_data.system)
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?,
            gpu_data: AgentRpcImpl::try_set_json(static_monitoring_data.gpu)
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?,
            data_hash: Set(data_hash),
        };

        debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Received static data");

        crate::monitoring_buffer::get()
            .static_mon
            .send(in_data)
            .map_err(|_| NodegetError::DatabaseError("Buffer closed".to_owned()))?;

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
