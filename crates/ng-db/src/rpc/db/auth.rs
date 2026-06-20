//! `db` 命名空间 RPC 的认证与名称校验
//!
//! 提供 `Token` 权限检查和数据库名称合法性校验，供 `create`/`read`/`update`/`delete`/`exec_sql` 复用。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Db as DbPermission, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;

/// 检查 Token 对指定数据库是否具有所需权限
///
/// - `token` — 原始 token 字符串（`key:secret` 或 `username|password` 格式）
/// - `db_name` — 目标数据库名称，构造 `Scope::Db(db_name)` 作用域
/// - `permission` — 所需的数据库权限级别（Create/Read/Update/Delete/ExecSql）
/// - 返回值：权限通过返回 `Ok(())`，否则返回 `PermissionDenied` 错误
///
/// # Errors
///
/// 当 Token 解析失败、认证提供者未初始化、权限检查失败或权限不足时返回错误
pub async fn check_db_permission(
    token: &str,
    db_name: &str,
    permission: DbPermission,
) -> anyhow::Result<()> {
    tracing::trace!(target: "db", db_name = db_name, "数据库权限检查: Db::{permission:?} on {db_name}");

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let provider =
        ng_core::permission::permission_checker::get_permission_checker().ok_or_else(|| {
            NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned())
        })?;

    let is_allowed = provider
        .check_token_limit(
            &token_or_auth,
            &[Scope::Db(db_name.to_owned())],
            &[Permission::Db(permission.clone())],
        )
        .await?;

    if !is_allowed {
        tracing::warn!(target: "db", db_name = db_name, "db permission denied for Db::{permission:?} on {db_name}");
        return Err(NodegetError::PermissionDenied(format!(
            "Permission Denied: Requires Db::{permission:?} on Scope::Db({db_name})"
        ))
        .into());
    }

    Ok(())
}

/// 校验数据库名称合法性
///
/// - `name` — 待校验的名称
/// - 返回值：合法返回 `Ok(())`，否则返回 `InvalidInput` 错误
///
/// 校验规则：
/// 1. 非空
/// 2. 长度不超过 128 字符
/// 3. 仅允许 ASCII 字母、数字、下划线、连字符、点号
/// 4. 不允许 `.` 或 `..`（防止路径遍历）
///
/// # Errors
///
/// 当名称为空、超长、包含非法字符或为路径遍历模式时返回 `InvalidInput` 错误
pub fn validate_db_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        return Err(NodegetError::InvalidInput("db name cannot be empty".to_owned()).into());
    }
    if name.len() > 128 {
        return Err(
            NodegetError::InvalidInput("db name too long (max 128 chars)".to_owned()).into(),
        );
    }
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if !valid {
        return Err(NodegetError::InvalidInput(
            "db name contains invalid characters (only [A-Za-z0-9_.-] allowed)".to_owned(),
        )
        .into());
    }
    if name == "." || name == ".." {
        return Err(NodegetError::InvalidInput("db name cannot be '.' or '..'".to_owned()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ng_core::error::NodegetError;

    // ── validate_db_name: happy path ──────────────────────────────

    #[test]
    fn validate_db_name_valid_simple() {
        assert!(validate_db_name("mydb").is_ok());
    }

    #[test]
    fn validate_db_name_valid_with_underscore() {
        assert!(validate_db_name("my_db").is_ok());
    }

    #[test]
    fn validate_db_name_valid_with_dash() {
        assert!(validate_db_name("my-db").is_ok());
    }

    #[test]
    fn validate_db_name_valid_with_dot() {
        assert!(validate_db_name("my.db").is_ok());
    }

    #[test]
    fn validate_db_name_valid_mixed() {
        assert!(validate_db_name("db_1-2.v3").is_ok());
    }

    #[test]
    fn validate_db_name_valid_all_alphanumeric() {
        assert!(validate_db_name("abc123").is_ok());
    }

    #[test]
    fn validate_db_name_valid_single_char() {
        assert!(validate_db_name("a").is_ok());
    }

    #[test]
    fn validate_db_name_valid_uppercase() {
        assert!(validate_db_name("DB_PROD").is_ok());
    }

    #[test]
    fn validate_db_name_accepts_exactly_128_chars() {
        let name = "a".repeat(128);
        assert!(validate_db_name(&name).is_ok());
    }

    // ── validate_db_name: empty ──────────────────────────────────

    #[test]
    fn validate_db_name_rejects_empty() {
        let result = validate_db_name("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("empty")));
    }

    // ── validate_db_name: too long ────────────────────────────────

    #[test]
    fn validate_db_name_rejects_too_long() {
        let long_name = "a".repeat(129);
        let result = validate_db_name(&long_name);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("128")));
    }

    // ── validate_db_name: path traversal ─────────────────────────

    #[test]
    fn validate_db_name_rejects_dot() {
        let result = validate_db_name(".");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(
            matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("'.'") || msg.contains("'..'"))
        );
    }

    #[test]
    fn validate_db_name_rejects_dotdot() {
        let result = validate_db_name("..");
        assert!(result.is_err());
    }

    // ── validate_db_name: invalid characters ──────────────────────

    #[test]
    fn validate_db_name_rejects_space() {
        assert!(validate_db_name("my db").is_err());
    }

    #[test]
    fn validate_db_name_rejects_slash() {
        assert!(validate_db_name("path/db").is_err());
    }

    #[test]
    fn validate_db_name_rejects_backslash() {
        assert!(validate_db_name("path\\db").is_err());
    }

    #[test]
    fn validate_db_name_rejects_asterisk() {
        assert!(validate_db_name("db*").is_err());
    }

    #[test]
    fn validate_db_name_rejects_unicode() {
        assert!(validate_db_name("数据库").is_err());
    }

    #[test]
    fn validate_db_name_rejects_special_chars() {
        for ch in [
            '!', '@', '#', '$', '%', '&', '(', ')', '=', '+', '[', ']', '{', '}', '|', ';', ':',
            '\'', '"', '<', '>', ',', '?', ' ', '/', '\\',
        ] {
            let name = format!("a{ch}b");
            assert!(
                validate_db_name(&name).is_err(),
                "expected rejection for char '{ch}'"
            );
        }
    }

    #[test]
    fn validate_db_name_dot_is_allowed_in_middle() {
        // Unlike ng-static which rejects all-dot segments, ng-db only rejects exact "." or ".."
        assert!(validate_db_name("v1.2.3").is_ok());
    }

    #[test]
    fn validate_db_name_all_dots_not_dot_or_dotdot() {
        // "..." is not "." or ".." but passes the char check (dots are allowed)
        assert!(validate_db_name("...").is_ok());
    }

    #[test]
    fn validate_db_name_rejects_leading_slash() {
        assert!(validate_db_name("/db").is_err());
    }
}
