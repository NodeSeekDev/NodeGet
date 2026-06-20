//! `crontab_result` RPC 权限校验：检查 CrontabResult 的读/删权限。
//!
//! 权限仅在 Global Scope 下生效，支持通配符 `*` 和特定 cron_name 两种粒度。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{CrontabResult, Permission, Scope};
use ng_core::permission::permission_checker::require_permission_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use tracing::{trace, warn};

/// 检查 CrontabResult 读权限。
///
/// 优先检查全局读权限（cron_name 为 `*`），再检查特定 cron_name 的读权限。
/// 该权限仅在 Global Scope 下有效。
///
/// - `token` - 认证 Token 字符串
/// - `cron_name` - 要读取的 cron_name
/// - 返回 Ok(()) 表示有权限，否则返回权限不足错误
pub async fn check_crontab_result_read_permission(
    token: &str,
    cron_name: &str,
) -> anyhow::Result<()> {
    trace!(target: "crontab_result", cron_name = %cron_name, "checking crontab_result read permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 Scope —— CrontabResult 权限仅在 Global Scope 下生效
    let scope = Scope::Global;
    let checker = require_permission_checker()?;

    // 先检查全局读权限（`*` 通配符表示所有 cron_name）
    let global_read_perm = Permission::CrontabResult(CrontabResult::Read("*".to_owned()));
    let has_global_read = checker
        .check_token_limit(&token_or_auth, std::slice::from_ref(&scope), &[global_read_perm])
        .await?;

    if has_global_read {
        return Ok(());
    }

    // 检查是否有特定 cron_name 的读权限
    let specific_read_perm = Permission::CrontabResult(CrontabResult::Read(cron_name.to_owned()));
    let has_specific_read = checker
        .check_token_limit(
            &token_or_auth,
            std::slice::from_ref(&scope),
            &[specific_read_perm],
        )
        .await?;

    if has_specific_read {
        return Ok(());
    }

    warn!(target: "crontab_result", cron_name = %cron_name, "read permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No read permission for crontab_result with cron_name '{cron_name}'"
    ))
    .into())
}

/// 检查 CrontabResult 删除权限。
///
/// 优先检查全局删除权限（cron_name 为 `*`），再检查特定 cron_name 的删除权限。
/// 若未指定 cron_name（None），仅检查全局权限。
/// 该权限仅在 Global Scope 下有效。
///
/// - `token` - 认证 Token 字符串
/// - `cron_name` - 要删除的 cron_name（None 表示检查全局权限）
/// - 返回 Ok(()) 表示有权限，否则返回权限不足错误
pub async fn check_crontab_result_delete_permission(
    token: &str,
    cron_name: Option<&str>,
) -> anyhow::Result<()> {
    trace!(target: "crontab_result", cron_name = ?cron_name, "checking crontab_result delete permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 Scope —— CrontabResult 权限仅在 Global Scope 下生效
    let scope = Scope::Global;
    let checker = require_permission_checker()?;

    // 检查全局删除权限
    let global_delete_perm = Permission::CrontabResult(CrontabResult::Delete("*".to_owned()));
    let has_global_delete = checker
        .check_token_limit(
            &token_or_auth,
            std::slice::from_ref(&scope),
            &[global_delete_perm],
        )
        .await?;

    if has_global_delete {
        return Ok(());
    }

    // 如果指定了 cron_name，检查特定权限
    if let Some(name) = cron_name {
        let specific_delete_perm =
            Permission::CrontabResult(CrontabResult::Delete(name.to_owned()));
        let has_specific_delete = checker
            .check_token_limit(
                &token_or_auth,
                std::slice::from_ref(&scope),
                &[specific_delete_perm],
            )
            .await?;

        if has_specific_delete {
            return Ok(());
        }
    }

    warn!(target: "crontab_result", cron_name = ?cron_name, "delete permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No delete permission for crontab_result with cron_name '{}'",
        cron_name.unwrap_or("*")
    ))
    .into())
}
