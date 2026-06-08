//! `static-bucket` 命名空间的 SuperToken 校验。
//!
//! `list` RPC 要求 SuperToken 权限，此处封装校验逻辑。

use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use tracing::{debug, trace, warn};

/// 检查给定 token 是否为 SuperToken。
///
/// - `token` - 完整的 Token 字符串
///
/// 返回：SuperToken 返回 `Ok(true)`，否则 `Ok(false)`；解析失败返回错误。
pub async fn check_super_token(token: &str) -> anyhow::Result<bool> {
    trace!(target: "static_bucket", "超级 Token 检查入口");
    let token_or_auth = TokenOrAuth::from_full_token(token).map_err(|e| {
        warn!(target: "static_bucket", "权限拒绝: Token 解析失败: {e}");
        NodegetError::ParseError(format!("Failed to parse token: {e}"))
    })?;
    let checker = ng_core::permission::permission_checker::get_permission_checker().ok_or_else(|| {
        NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned())
    })?;
    let result = checker
        .check_super_token(&token_or_auth)
        .await
        .map_err(|e| {
            warn!(target: "static_bucket", "权限拒绝: Super Token 检查失败: {e}");
            NodegetError::PermissionDenied(format!("{e}"))
        })?;
    if result {
        debug!(target: "static_bucket", "超级 Token 检查通过");
    } else {
        warn!(target: "static_bucket", "权限拒绝: 非 Super Token 尝试访问");
    }
    Ok(result)
}
