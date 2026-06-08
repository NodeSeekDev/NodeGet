//! `nodeget-server::exec_sql` RPC 实现 — 在主库上执行 SQL

use crate::db_registry::{json_to_sea_value, row_to_json};
use crate::rpc::{to_rpc_error, token_identity};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{NodeGet as NodeGetPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use sea_orm::{ConnectionTrait, Statement};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, warn};

/// 在主库上执行 SQL 语句，需要 `NodeGet::ExecSql` 权限（Global 作用域）
///
/// - `token` — 认证 Token
/// - `sql` — SQL 语句
/// - `params` — 参数数组或 null
/// - 返回值：包含 `data`、`row_count`、`truncated` 的响应
///
/// 内部步骤：
/// 1. 解析 Token 并检查 `NodeGet::ExecSql` 权限
/// 2. 从全局单例获取主库连接
/// 3. 解析参数为 `SeaORM` `Value` 数组
/// 4. 执行 SQL 并收集结果行
/// 5. 超过 10000 行时截断并标记 `truncated: true`
///
/// # Errors
///
/// 当 Token 解析失败、认证提供者未初始化、权限不足、数据库未初始化或 SQL 执行失败时返回错误
pub async fn exec_sql(
    token: String,
    sql: String,
    params: Option<Value>,
) -> RpcResult<Box<RawValue>> {
    let (tk, un) = token_identity(&token);
    debug!(target: "nodeget", token_key = tk, username = un, sql_len = sql.len(), "exec_sql called");

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
            warn!(target: "nodeget", token_key = tk, username = un, "exec_sql permission denied");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: missing nodeget.exec_sql permission".to_owned(),
            )
            .into());
        }

        let db = crate::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("Database not initialized".to_owned()))?;

        let db_backend = db.get_database_backend();
        let sea_values = match params {
            Some(Value::Array(arr)) => arr.iter().map(json_to_sea_value).collect(),
            Some(Value::Null) | None => vec![],
            _ => {
                return Err(NodegetError::InvalidInput(
                    "params must be an array or null".to_owned(),
                )
                .into());
            }
        };

        let stmt = Statement::from_sql_and_values(db_backend, &sql, sea_values);

        let mut rows = db.query_all_raw(stmt).await?;
        let total_count = rows.len() as u64;
        let truncated = rows.len() > 10_000;
        if truncated {
            rows.truncate(10_000);
        }
        let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();

        let response = serde_json::json!({
            "success": true,
            "data": json_rows,
            "row_count": total_count,
            "truncated": truncated,
        });

        serde_json::value::to_raw_value(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => Err(to_rpc_error(&e)),
    }
}
