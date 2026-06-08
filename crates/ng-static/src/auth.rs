//! 静态文件桶（Static Bucket）权限校验模块。
//!
//! 职责：对 `static-bucket` 和 `static-bucket-file` 两个 RPC 命名空间
//! 的调用方进行 Scope + Permission 级别的细粒度鉴权。
//!
//! 权限校验委托至全局 `ng_core::permission::permission_checker::PermissionChecker`。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    Permission, Scope, StaticBucket as StaticBucketPermission,
    StaticBucketFile as StaticBucketFilePermission,
};
use ng_core::permission::permission_checker::require_permission_checker as get_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use tracing::{trace, warn};

// ── 静态文件桶权限检查 ──────────────────────────────────────────────

/// 校验指定 Token 是否拥有对某个静态文件桶的特定操作权限。
///
/// - `token` - 完整的 Token 字符串（key:secret 或 username|password 格式）
/// - `name` - 目标静态文件桶名称，同时作为 Scope 的标识
/// - `permission` - 需要校验的 [`StaticBucketPermission`] 操作类型
///
/// 返回：权限通过返回 `Ok(())`，否则返回 `PermissionDenied` 错误。
pub async fn check_static_bucket_permission(
    token: &str,
    name: &str,
    permission: StaticBucketPermission,
) -> anyhow::Result<()> {
    trace!(target: "static_bucket", name = %name, permission = ?permission, "checking static-bucket permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let checker = get_checker()?;
    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::StaticBucket(name.to_owned())],
            vec![Permission::StaticBucket(permission)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "static_bucket", name = %name, permission = %permission_name, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for static-bucket '{name}', required permission: {permission_name}"
    ))
    .into())
}

/// 校验指定 Token 是否拥有对某个静态文件桶内文件的特定操作权限。
///
/// - `token` - 完整的 Token 字符串（key:secret 或 username|password 格式）
/// - `name` - 目标静态文件桶名称，同时作为 Scope 的标识
/// - `permission` - 需要校验的 [`StaticBucketFilePermission`] 操作类型
///
/// 返回：权限通过返回 `Ok(())`，否则返回 `PermissionDenied` 错误。
pub async fn check_static_bucket_file_permission(
    token: &str,
    name: &str,
    permission: StaticBucketFilePermission,
) -> anyhow::Result<()> {
    trace!(target: "static_bucket_file", name = %name, permission = ?permission, "checking static-bucket-file permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let checker = get_checker()?;
    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::StaticBucket(name.to_owned())],
            vec![Permission::StaticBucketFile(permission)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "static_bucket_file", name = %name, permission = %permission_name, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for static-bucket-file '{name}', required permission: {permission_name}"
    ))
    .into())
}
