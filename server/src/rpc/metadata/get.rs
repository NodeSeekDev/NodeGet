use crate::entity::metadata as metadata_entity;
use crate::rpc::RpcHelper;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::metadata;
use nodeget_lib::permission::data_structure::{Metadata as MetadataPermission, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use uuid::Uuid;

pub async fn get(token: String, uuid: Uuid) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(uuid)],
            vec![Permission::Metadata(MetadataPermission::Read)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing Metadata Read permission".to_owned(),
            )
            .into());
        }

        let db = <super::MetadataRpcImpl as RpcHelper>::get_db()?;

        let metadata_struct = match metadata_entity::Entity::find()
            .filter(metadata_entity::Column::Uuid.eq(uuid))
            .one(db)
            .await
        {
            Ok(Some(model)) => metadata::Metadata {
                agent_uuid: model.uuid,
                agent_name: model.name,
                agent_tags: model.tags.map_or_else(std::vec::Vec::new, |json_val| {
                    serde_json::from_value(json_val).unwrap_or_else(|_| vec![])
                }),
            },
            Ok(None) => metadata::Metadata {
                agent_uuid: uuid,
                agent_name: String::new(),
                agent_tags: vec![],
            },
            Err(e) => return Err(NodegetError::DatabaseError(format!("Database error: {e}")).into()),
        };

        let json_str = serde_json::to_string(&metadata_struct)
            .map_err(|e| NodegetError::SerializationError(format!("Serialization failed: {e}")))?;

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
