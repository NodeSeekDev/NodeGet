use crate::entity::db_registry;
use crate::rpc::db::auth::check_db_permission;
use crate::rpc::{to_rpc_error, token_identity};
use crate::{db_registry::DbRegistryManager, get_db};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Db as DbPermission;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        check_db_permission(&token, &name, DbPermission::Delete).await?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        let model = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        model.ok_or_else(|| NodegetError::NotFound(format!("Database '{name}' not found")))?;

        // remove_conn handles: pool removal, registry deletion, file cleanup (.db, -wal, -shm)
        let mgr = DbRegistryManager::global();
        mgr.remove_conn(&name)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to delete database: {e}")))?;

        debug!(target: "db", token_key = tk, username = un, name = %name, "database deleted");

        let resp = serde_json::json!({
            "success": true,
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
