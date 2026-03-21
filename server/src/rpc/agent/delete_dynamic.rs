use crate::entity::dynamic_monitoring;
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use uuid::Uuid;

pub async fn delete_dynamic(
    token: String,
    agent_uuid: Uuid,
    before_timestamp: i64,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::DynamicMonitoring(DynamicMonitoring::Delete)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Delete permission for this Agent"
                    .to_owned(),
            )
            .into());
        }

        let db = AgentRpcImpl::get_db()?;

        let result = dynamic_monitoring::Entity::delete_many()
            .filter(dynamic_monitoring::Column::Uuid.eq(agent_uuid))
            .filter(dynamic_monitoring::Column::Timestamp.lt(before_timestamp))
            .exec(db)
            .await
            .map_err(|e| {
                error!("Database delete error: {e}");
                NodegetError::DatabaseError(format!("Database delete error: {e}"))
            })?;

        let json_str = format!(
            "{{\"success\":true,\"deleted\":{},\"agent_uuid\":\"{}\",\"before_timestamp\":{}}}",
            result.rows_affected, agent_uuid, before_timestamp
        );
        RawValue::from_string(json_str)
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

