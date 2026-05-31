use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    Permission, Scope, StaticBucket as StaticBucketPermission,
    StaticBucketFile as StaticBucketFilePermission,
};
use ng_core::permission::token_auth::TokenOrAuth;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use tracing::{trace, warn};

// ── TokenPermissionChecker trait + global injection ────────────────────

/// Trait for token permission checking operations needed by static auth.
///
/// The server crate must implement this trait and inject it via
/// [`set_token_checker`] during startup.
pub trait TokenPermissionChecker: Send + Sync {
    /// Check if the token/auth satisfies the given scopes and permissions.
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send + '_>>;

    /// Check if the token/auth is a super token.
    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send + '_>>;
}

static TOKEN_CHECKER: OnceLock<Box<dyn TokenPermissionChecker>> = OnceLock::new();

/// Set the global token permission checker.
///
/// Must be called once during server startup.
pub fn set_token_checker(checker: Box<dyn TokenPermissionChecker>) {
    let _ = TOKEN_CHECKER.set(checker);
}

/// Get the global token permission checker.
///
/// Panics if not initialized -- call [`set_token_checker`] first.
pub fn get_token_checker() -> &'static dyn TokenPermissionChecker {
    TOKEN_CHECKER
        .get()
        .expect("TokenPermissionChecker not initialized -- call set_token_checker first")
        .as_ref()
}

// ── Static bucket permission checks ────────────────────────────────────

pub async fn check_static_bucket_permission(
    token: &str,
    name: &str,
    permission: StaticBucketPermission,
) -> anyhow::Result<()> {
    trace!(target: "static_bucket", name = %name, permission = ?permission, "checking static-bucket permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let is_allowed = get_token_checker()
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

pub async fn check_static_bucket_file_permission(
    token: &str,
    name: &str,
    permission: StaticBucketFilePermission,
) -> anyhow::Result<()> {
    trace!(target: "static_bucket_file", name = %name, permission = ?permission, "checking static-bucket-file permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let is_allowed = get_token_checker()
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
