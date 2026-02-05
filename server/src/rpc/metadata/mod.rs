use crate::entity::metadata as metadata_entity;
use crate::rpc::RpcHelper;
use crate::token::get::check_token_limit;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::metadata;
use nodeget_lib::permission::data_structure::{Metadata as MetadataPermission, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::error_message::generate_error_message;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[rpc(server, namespace = "metadata")]
pub trait Rpc {
    #[method(name = "get")]
    async fn get(&self, token: String, uuid: Uuid) -> Value;

    #[method(name = "write")]
    async fn write(&self, token: String, metadata: metadata::Metadata) -> Value;
}

pub struct MetadataRpcImpl;

impl RpcHelper for MetadataRpcImpl {}

#[async_trait]
impl RpcServer for MetadataRpcImpl {
    async fn get(&self, token: String, uuid: Uuid) -> Value {
        let process_logic = async {
            let token_or_auth = match TokenOrAuth::from_full_token(&token) {
                Ok(toa) => toa,
                Err(e) => {
                    return Err((101, format!("Failed to parse token: {e}")));
                }
            };

            let is_allowed = check_token_limit(
                &token_or_auth,
                vec![Scope::AgentUuid(uuid)],
                vec![Permission::Metadata(MetadataPermission::Read)],
            )
            .await?;

            if !is_allowed {
                return Err((
                    102,
                    "Permission Denied: Missing Metadata Read permission".to_string(),
                ));
            }

            let db = Self::get_db()?;

            match metadata_entity::Entity::find()
                .filter(metadata_entity::Column::Uuid.eq(uuid))
                .one(db)
                .await
            {
                Ok(Some(model)) => {
                    let metadata_struct = metadata::Metadata {
                        agent_uuid: model.uuid,
                        agent_name: model.name,
                        agent_tags: model.tags.map_or_else(std::vec::Vec::new, |json_val| {
                            serde_json::from_value(json_val).unwrap_or_else(|_| vec![])
                        }),
                    };
                    Ok(metadata_struct)
                }
                Ok(None) => {
                    let empty_metadata = metadata::Metadata {
                        agent_uuid: uuid,
                        agent_name: String::new(),
                        agent_tags: vec![],
                    };
                    Ok(empty_metadata)
                }
                _ => Err((103, "Database error".to_string())),
            }
        };

        match process_logic.await {
            Ok(metadata) => serde_json::to_value(metadata).unwrap_or_else(|_| json!({})),
            Err((code, msg)) => generate_error_message(code, &msg),
        }
    }

    async fn write(&self, token: String, metadata: metadata::Metadata) -> Value {
        let process_logic = async {
            let token_or_auth = match TokenOrAuth::from_full_token(&token) {
                Ok(toa) => toa,
                Err(e) => {
                    return Err((101, format!("Failed to parse token: {e}")));
                }
            };

            let is_allowed = match check_token_limit(
                &token_or_auth,
                vec![Scope::AgentUuid(metadata.agent_uuid)],
                vec![Permission::Metadata(MetadataPermission::Write)],
            )
            .await
            {
                Ok(result) => result,
                Err((code, msg)) => {
                    return Err((code, msg));
                }
            };

            if !is_allowed {
                return Err((
                    102,
                    "Permission Denied: Missing Metadata Write permission".to_string(),
                ));
            }

            let db = Self::get_db()?;

            let tags_json = match serde_json::to_value(&metadata.agent_tags) {
                Ok(json_val) => Some(json_val),
                Err(e) => {
                    return Err((101, format!("Failed to serialize tags: {e}")));
                }
            };

            // Check if metadata record already exists
            match metadata_entity::Entity::find()
                .filter(metadata_entity::Column::Uuid.eq(metadata.agent_uuid))
                .one(db)
                .await
            {
                Ok(Some(existing_model)) => {
                    let mut active_model: metadata_entity::ActiveModel = existing_model.into();
                    active_model.name = Set(metadata.agent_name);
                    active_model.tags = Set(tags_json);

                    match active_model.clone().update(db).await {
                        Ok(_) => Ok(active_model.id.unwrap()),
                        Err(e) => Err((103, format!("Database update error: {e}"))),
                    }
                }
                Ok(None) => {
                    let new_metadata = metadata_entity::ActiveModel {
                        id: ActiveValue::default(),
                        uuid: Set(metadata.agent_uuid),
                        name: Set(metadata.agent_name),
                        tags: Set(tags_json),
                    };

                    match new_metadata.insert(db).await {
                        Ok(inserted_model) => Ok(inserted_model.id),
                        Err(e) => Err((103, format!("Database insert error: {e}"))),
                    }
                }
                _ => Err((103, "Database error".to_string())),
            }
        };

        match process_logic.await {
            Ok(id) => json!({"id": id}),
            Err((code, msg)) => generate_error_message(code, &msg),
        }
    }
}
