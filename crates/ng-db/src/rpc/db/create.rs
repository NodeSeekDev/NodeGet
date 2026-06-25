//! `db.create` RPC 实现 — 创建用户数据库

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

/// 创建新的用户数据库
///
/// - `token` — 认证 Token
/// - `name` — 新数据库名称
/// - 返回值：包含 `name` 和 `file_path` 的成功响应
///
/// 内部步骤：
/// 1. 检查 `Db::Create` 权限
/// 2. 校验数据库名称合法性
/// 3. 检查名称是否已存在于 `db_registry`
/// 4. 通过 `DbRegistryManager` 创建 `SQLite` 文件并注册
pub async fn create(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        // 先校验名称合法性，再做鉴权：非法名称对无权限调用方应返回 108
        // （InvalidInput）而非 102（PermissionDenied）；且避免用未校验的 name
        // 构造 Scope 做无意义的权限查询。
        validate_db_name(&name)?;
        check_db_permission(&token, &name, DbPermission::Create).await?;

        let db = get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Main database not initialized".to_owned())
        })?;

        // 检查数据库名称是否已存在于注册表
        let existing = db_registry::Entity::find()
            .filter(db_registry::Column::Name.eq(&name))
            .one(db)
            .await?;

        if existing.is_some() {
            return Err(
                NodegetError::InvalidInput(format!("Database '{name}' already exists")).into(),
            );
        }

        // 通过 DbRegistryManager 创建 SQLite 文件并注册到 db_registry 表
        let mgr = DbRegistryManager::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
        })?;
        let _conn = mgr.create_conn(&name, None).await?;

        debug!(target: "db", token_key = tk, username = un, name = %name, "database created");

        let resp = serde_json::json!({
            "success": true,
            "data": {
                "name": name,
                "file_path": mgr.get_db_path(&name),
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
