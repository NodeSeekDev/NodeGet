use crate::token::get::check_token_limit;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{CrontabResult, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;

/// 检查是否有 `CrontabResult` 读权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `cron_name` - 要读取的 `cron_name`
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
///
/// # 注意
/// 该权限仅在 Global Scope 下有效
pub async fn check_crontab_result_read_permission(
    token: &str,
    cron_name: &str,
) -> anyhow::Result<()> {
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 Global
    let scope = Scope::Global;

    // 先检查是否有全局读权限（cron_name 为 "*" 表示所有 cron_name）
    let global_read_perm = Permission::CrontabResult(CrontabResult::Read("*".to_owned()));
    let has_global_read =
        check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_read_perm]).await?;

    if has_global_read {
        return Ok(());
    }

    // 检查是否有特定 cron_name 的读权限
    let specific_read_perm = Permission::CrontabResult(CrontabResult::Read(cron_name.to_owned()));
    let has_specific_read = check_token_limit(
        &token_or_auth,
        vec![scope.clone()],
        vec![specific_read_perm],
    )
    .await?;

    if has_specific_read {
        return Ok(());
    }

    Err(NodegetError::PermissionDenied(format!(
        "No read permission for crontab_result with cron_name '{cron_name}'"
    ))
    .into())
}

/// 检查是否有 `CrontabResult` 删除权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `cron_name` - 要删除的 `cron_name（可选，None` 表示删除所有）
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
///
/// # 注意
/// 该权限仅在 Global Scope 下有效
pub async fn check_crontab_result_delete_permission(
    token: &str,
    cron_name: Option<&str>,
) -> anyhow::Result<()> {
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 Global
    let scope = Scope::Global;

    // 检查是否有全局删除权限
    let global_delete_perm = Permission::CrontabResult(CrontabResult::Delete("*".to_owned()));
    let has_global_delete = check_token_limit(
        &token_or_auth,
        vec![scope.clone()],
        vec![global_delete_perm],
    )
    .await?;

    if has_global_delete {
        return Ok(());
    }

    // 如果指定了 cron_name，检查特定权限
    if let Some(name) = cron_name {
        let specific_delete_perm =
            Permission::CrontabResult(CrontabResult::Delete(name.to_owned()));
        let has_specific_delete = check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![specific_delete_perm],
        )
        .await?;

        if has_specific_delete {
            return Ok(());
        }

    }

    Err(NodegetError::PermissionDenied(format!(
        "No delete permission for crontab_result with cron_name '{}'",
        cron_name.unwrap_or("*")
    ))
    .into())
}
