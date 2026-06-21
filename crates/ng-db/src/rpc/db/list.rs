//! `db.list` RPC 实现 — 列出所有用户数据库

use crate::db_registry::DbRegistryManager;
use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Db as DbPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::{debug, warn};

/// 列出所有已注册数据库
///
/// - `token` — 认证 Token
/// - 返回值：包含所有 `DbInfo` 的列表
///
/// 内部步骤：
/// 1. 解析 Token 并检查 `Db::List` 权限（Global 作用域）
/// 2. 通过 `DbRegistryManager::list_all` 获取所有数据库信息
pub async fn list(token: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);

    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = ng_core::permission::permission_checker::get_permission_checker()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned())
            })?;

        let is_allowed = provider
            .check_token_limit(
                &token_or_auth,
                &[Scope::Global],
                &[Permission::Db(DbPermission::List)],
            )
            .await?;

        if !is_allowed {
            warn!(target: "db", "权限拒绝: Db::List in Global scope");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Requires Db::List in Global scope".to_owned(),
            )
            .into());
        }

        let mgr = DbRegistryManager::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("DbRegistryManager not initialized".to_owned())
        })?;
        let all = mgr.list_all().await?;

        debug!(target: "db", token_key = tk, username = un, count = all.len(), "database list");

        let resp = serde_json::json!({
            "success": true,
            "data": all,
        });

        serde_json::value::to_raw_value(&resp)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
