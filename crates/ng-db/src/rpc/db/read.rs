//! `db.read` RPC 实现 — 查询单个用户数据库信息

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

/// 查询指定数据库的详细信息
///
/// - `token` — 认证 Token
/// - `name` — 数据库名称
/// - 返回值：包含 `id`、`name`、`created_at`、`active` 的响应
///
/// 内部步骤：
/// 1. 检查 `Db::Read` 权限
/// 2. 从 `db_registry` 表查询条目
/// 3. 检查连接池中该数据库是否活跃
pub async fn read(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        check_db_permission(&token, &name, DbPermission::Read).await?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        let model = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        let model =
            model.ok_or_else(|| NodegetError::NotFound(format!("Database '{name}' not found")))?;

        let mgr = DbRegistryManager::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
        })?;
        let is_active = mgr.has_conn(&name);

        debug!(target: "db", token_key = tk, username = un, name = %name, "database read");

        let resp = serde_json::json!({
            "success": true,
            "data": {
                "id": model.id,
                "name": model.name,
                "created_at": model.created_at,
                "active": is_active,
            }
        });

        serde_json::value::to_raw_value(&resp)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
