//! `nodeget-server::get_database_type` RPC 实现 — 查询主库类型

use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{NodeGet as NodeGetPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use sea_orm::DbBackend;
use serde_json::value::RawValue;
use tracing::warn;

/// 查询主库数据库类型，需要 `NodeGet::ExecSql` 权限（复用该权限，Global 作用域）
///
/// - `token` — 认证 Token
/// - 返回值：`{"success": true, "data": "sqlite"|"postgres"|"mysql"|"unknown"}`
///
/// 内部步骤：
/// 1. 解析 Token 并检查 `NodeGet::ExecSql` 权限
/// 2. 从全局单例获取主库连接
/// 3. 根据 `get_database_backend()` 返回数据库类型字符串
///
/// # Errors
///
/// 当 Token 解析失败、认证提供者未初始化、权限不足或数据库未初始化时返回错误
pub async fn get_database_type(token: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);
    tracing::debug!(target: "nodeget", token_key = tk, username = un, "get_database_type called");

    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let provider = ng_core::permission::permission_checker::get_permission_checker()
            .ok_or_else(|| NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned()))?;

        let is_allowed = provider
            .check_token_limit(
                &token_or_auth,
                vec![Scope::Global],
                vec![Permission::NodeGet(NodeGetPermission::ExecSql)],
            )
            .await?;

        if !is_allowed {
            warn!(target: "nodeget", token_key = tk, username = un, "get_database_type permission denied");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: missing nodeget.exec_sql permission".to_owned(),
            )
            .into());
        }

        let db = crate::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("Database not initialized".to_owned()))?;

        let db_type = match db.get_database_backend() {
            DbBackend::Sqlite => "sqlite",
            DbBackend::Postgres => "postgres",
            DbBackend::MySql => "mysql",
            _ => "unknown",
        };

        let response = serde_json::json!({
            "success": true,
            "data": db_type,
        });

        serde_json::value::to_raw_value(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
