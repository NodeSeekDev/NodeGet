//! `db.delete` RPC 实现 — 删除用户数据库

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

/// 删除指定用户数据库
///
/// - `token` — 认证 Token
/// - `name` — 待删除的数据库名称
/// - 返回值：`{"success": true}` 响应
///
/// 内部步骤：
/// 1. 检查 `Db::Delete` 权限
/// 2. 校验数据库名称合法性
/// 3. 确认数据库存在于 `db_registry`
/// 4. 通过 `remove_conn` 移除连接池条目、删除注册表行和磁盘文件
pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        // 先校验名称合法性，再做鉴权（见 create.rs 同款注释）。
        validate_db_name(&name)?;
        check_db_permission(&token, &name, DbPermission::Delete).await?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        let model = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        model.ok_or_else(|| NodegetError::NotFound(format!("Database '{name}' not found")))?;

        // remove_conn 内部处理：移除连接池条目、删除注册表行、清理 .db/-wal/-shm 文件
        let mgr = DbRegistryManager::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
        })?;
        mgr.remove_conn(&name)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to delete database: {e}")))?;

        debug!(target: "db", token_key = tk, username = un, name = %name, "database deleted");

        let resp = serde_json::json!({
            "success": true,
        });

        serde_json::value::to_raw_value(&resp)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
