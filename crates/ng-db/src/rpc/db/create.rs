use crate::entity::db_registry;
use crate::rpc::db::auth::{check_db_permission, validate_db_name};
use crate::rpc::{to_rpc_error, token_identity};
use crate::{db_registry::DbRegistryManager, get_db};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Db as DbPermission;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn create(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        check_db_permission(&token, &name, DbPermission::Create).await?;
        validate_db_name(&name)?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        // Check if db name already exists in registry
        let existing = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        if existing.is_some() {
            return Err(
                NodegetError::InvalidInput(format!("Database '{name}' already exists")).into(),
            );
        }

        // Initialize the SQLite file and create registry entry via DbRegistryManager
        let mgr = DbRegistryManager::global();
        let _conn = mgr.create_conn(&name, None).await?;

        debug!(target: "db", token_key = tk, username = un, name = %name, "database created");

        let resp = serde_json::json!({
            "success": true,
            "data": {
                "name": name,
                "file_path": mgr.get_db_path(&name),
            }
        });

        let json_str = serde_json::to_string(&resp)?;
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
