//! `db.update` RPC 实现 — 重命名用户数据库
//!
//! 重命名采用三阶段提交策略：先改磁盘文件、再改注册表、最后刷新连接池，
//! 若注册表更新失败则回滚文件重命名。

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

/// 重命名用户数据库（文件 + 注册表 + 连接池同步更新）
///
/// - `token` — 认证 Token
/// - `name` — 当前数据库名称
/// - `new_name` — 新数据库名称
/// - 返回值：包含更新后 `id`、`name`、`created_at` 的响应
///
/// 内部步骤：
/// 1. 检查新旧名称的 `Db::Update` 权限，校验新名称合法性
/// 2. 确认旧名称存在、新名称不存在
/// 3. 在 `spawn_blocking` 中重命名磁盘文件（.db、-wal、-shm）
/// 4. 更新 `db_registry` 表中的名称
/// 5. 若注册表更新失败，回滚文件重命名
/// 6. 刷新连接池：移除旧连接、创建新连接
#[allow(clippy::too_many_lines)]
pub async fn update(token: String, name: String, new_name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        // 先校验新名称合法性，再做鉴权（见 create.rs 同款注释）。
        validate_db_name(&new_name)?;
        // 新旧名称均需 Update 权限
        check_db_permission(&token, &name, DbPermission::Update).await?;
        check_db_permission(&token, &new_name, DbPermission::Update).await?;

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

        let mgr = DbRegistryManager::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
        })?;
        let old_file = mgr.get_db_path(&name);
        let new_file = mgr.get_db_path(&new_name);

        // 阶段 1：先重命名磁盘文件。若此步失败，数据库状态未变
        let old_file_clone = old_file.clone();
        let new_file_clone = new_file.clone();
        let (rename_result, wal_warnings) = tokio::task::spawn_blocking(
            move || -> (Result<(), NodegetError>, Vec<(String, String)>) {
                let mut warnings = Vec::new();
                if std::path::Path::new(&old_file_clone).exists() {
                    match std::fs::rename(&old_file_clone, &new_file_clone) {
                        Ok(()) => {
                            for ext in &["-wal", "-shm"] {
                                let old_ext = format!("{old_file_clone}{ext}");
                                let new_ext = format!("{new_file_clone}{ext}");
                                if std::path::Path::new(&old_ext).exists()
                                    && std::fs::rename(&old_ext, &new_ext).is_err()
                                {
                                    warnings.push((old_ext, new_ext));
                                }
                            }
                        }
                        Err(e) => {
                            return (
                                Err(NodegetError::IoError(format!(
                                    "Failed to rename db file: {e}"
                                ))),
                                warnings,
                            );
                        }
                    }
                }
                (Ok(()), warnings)
            },
        )
        .await
        .map_err(|e| NodegetError::Other(format!("spawn_blocking failed: {e}")))?;
        for (old_ext, new_ext) in wal_warnings {
            warn!(target: "db", old = %old_ext, new = %new_ext,
                "Failed to rename WAL/SHM file, SQLite will recover on next open");
        }
        rename_result?;

        // 阶段 2：更新注册表行。若此步失败，回滚文件重命名
        let update_result = {
            let mut active: db_registry::ActiveModel = model.into();
            active.name = Set(new_name.clone());
            active.update(db).await
        };

        match update_result {
            Ok(updated) => {
                // 阶段 3：刷新连接池。若此步失败，DB 和文件已一致，仅输出警告不回滚
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

                serde_json::value::to_raw_value(&resp)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
            }
            Err(e) => {
                // 注册表更新失败，回滚文件重命名
                let new_file_rb = new_file.clone();
                let old_file_rb = old_file.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if std::path::Path::new(&new_file_rb).exists() {
                        let _ = std::fs::rename(&new_file_rb, &old_file_rb);
                        for ext in &["-wal", "-shm"] {
                            let new_ext = format!("{new_file_rb}{ext}");
                            let old_ext = format!("{old_file_rb}{ext}");
                            if std::path::Path::new(&new_ext).exists() {
                                let _ = std::fs::rename(&new_ext, &old_ext);
                            }
                        }
                    }
                })
                .await;
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
