use crate::entity::db_registry;
use crate::rpc::db::auth::{check_db_permission, validate_db_name};
use crate::rpc::{to_rpc_error, token_identity};
use crate::{db_registry::DbRegistryManager, get_db};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Db as DbPermission;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::{debug, warn};

pub async fn update(token: String, name: String, new_name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        // Require permission on both old and new db names
        check_db_permission(&token, &name, DbPermission::Update).await?;
        check_db_permission(&token, &new_name, DbPermission::Update).await?;
        validate_db_name(&new_name)?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        let model = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        let model =
            model.ok_or_else(|| NodegetError::NotFound(format!("Database '{name}' not found")))?;

        let existing = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&new_name))
            .one(db)
            .await?;
        if existing.is_some() {
            return Err(NodegetError::InvalidInput(format!(
                "Database '{new_name}' already exists"
            ))
            .into());
        }

        let mgr = DbRegistryManager::global();
        let old_file = mgr.get_db_path(&name);
        let new_file = mgr.get_db_path(&new_name);

        // Step 1: Rename files on disk first (atomic operation).
        // If this fails, no database state has changed.
        if std::path::Path::new(&old_file).exists() {
            std::fs::rename(&old_file, &new_file)
                .map_err(|e| NodegetError::IoError(format!("Failed to rename db file: {e}")))?;
            for ext in &["-wal", "-shm"] {
                let old_ext = format!("{old_file}{ext}");
                let new_ext = format!("{new_file}{ext}");
                if std::path::Path::new(&old_ext).exists() {
                    // Best effort: if WA file rename fails, the main file is already
                    // renamed so we continue. SQLite will recover WAL on next open.
                    if std::fs::rename(&old_ext, &new_ext).is_err() {
                        warn!(target: "db", old = %old_ext, new = %new_ext,
                            "Failed to rename WAL/SHM file, SQLite will recover on next open");
                    }
                }
            }
        }

        // Step 2: Update database registry row.
        // If this fails, rollback the file rename.
        let update_result = {
            let mut active: db_registry::ActiveModel = model.into();
            active.name = Set(new_name.clone());
            active.update(db).await
        };

        match update_result {
            Ok(updated) => {
                // Step 3: Update pool entries.
                // If pool operation fails, DB + files are already consistent,
                // so we log the warning but don't fail the overall operation.
                let _ = mgr.remove_conn(&name).await;
                match mgr.create_conn(&new_name, updated.max_lifetime_ms).await {
                    Ok(_) => {}
                    Err(e) => {
                        warn!(target: "db", name = %new_name, error = %e,
                            "Pool connection creation failed after rename, caller should create_conn manually");
                    }
                }

                debug!(target: "db", token_key = tk, username = un, name = %name, new_name = %new_name, "database renamed");

                let resp = serde_json::json!({
                    "success": true,
                    "data": {
                        "id": updated.id,
                        "name": updated.name,
                        "created_at": updated.created_at,
                    }
                });

                let json_str = serde_json::to_string(&resp)?;
                RawValue::from_string(json_str)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
            }
            Err(e) => {
                // Rollback file rename
                if std::path::Path::new(&new_file).exists() {
                    let _ = std::fs::rename(&new_file, &old_file);
                    for ext in &["-wal", "-shm"] {
                        let new_ext = format!("{new_file}{ext}");
                        let old_ext = format!("{old_file}{ext}");
                        if std::path::Path::new(&new_ext).exists() {
                            let _ = std::fs::rename(&new_ext, &old_ext);
                        }
                    }
                }
                Err(NodegetError::DatabaseError(format!(
                    "Failed to update registry after rename: {e}"
                ))
                .into())
            }
        }
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
