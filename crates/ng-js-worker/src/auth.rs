use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    JsResult as JsResultPermission, JsWorker as JsWorkerPermission, NodeGet, Permission, Scope,
    Token,
};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::js_result;
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use tracing::{trace, warn};

// ── TokenPermissionChecker trait + global injection ────────────────────

/// Trait for token permission checking operations needed by JS worker auth.
///
/// The server crate must implement this trait and inject it via
/// [`set_token_checker`] during startup.
pub trait TokenPermissionChecker: Send + Sync + 'static {
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

    /// Get token metadata for the given token/auth.
    fn get_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Token>> + Send + '_>>;
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
/// Panics if not initialized — call [`set_token_checker`] first.
pub fn get_token_checker() -> &'static dyn TokenPermissionChecker {
    TOKEN_CHECKER
        .get()
        .expect("TokenPermissionChecker not initialized — call set_token_checker first")
        .as_ref()
}

// ── js_worker permission helpers ───────────────────────────────────────

pub async fn check_js_worker_permission(
    token: &str,
    worker_name: &str,
    permission: JsWorkerPermission,
) -> anyhow::Result<()> {
    trace!(target: "js_worker", worker_name = %worker_name, permission = ?permission, "checking js_worker permission");
    let checker = get_token_checker();
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::JsWorker(worker_name.to_owned())],
            vec![Permission::JsWorker(permission)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_worker", worker_name = %worker_name, permission = %permission_name, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for js_worker '{worker_name}', required permission: {permission_name}"
    ))
    .into())
}

pub async fn check_get_rt_pool_permission(token: &str) -> anyhow::Result<()> {
    trace!(target: "js_worker", "checking get_rt_pool permission");
    let checker = get_token_checker();
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::NodeGet(NodeGet::GetRtPool)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_worker", "get_rt_pool permission denied");
    Err(NodegetError::PermissionDenied(
        "Permission denied: missing nodeget.get_rt_pool permission".to_owned(),
    )
    .into())
}

pub async fn filter_workers_by_list_permission(
    token: &str,
    worker_names: Vec<String>,
) -> anyhow::Result<Vec<String>> {
    trace!(target: "js_worker", count = worker_names.len(), "filtering workers by list permission");
    let checker = get_token_checker();
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let mut allowed = Vec::new();
    for worker_name in worker_names {
        let is_allowed = checker
            .check_token_limit(
                &token_or_auth,
                vec![Scope::JsWorker(worker_name.clone())],
                vec![Permission::JsWorker(JsWorkerPermission::ListAllJsWorker)],
            )
            .await?;

        if is_allowed {
            allowed.push(worker_name);
        }
    }

    Ok(allowed)
}

// ── js_result permission helpers ───────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum JsResultAction {
    Read,
    Delete,
}

fn build_required_permission(action: JsResultAction, worker_name: &str) -> Permission {
    match action {
        JsResultAction::Read => {
            Permission::JsResult(JsResultPermission::Read(worker_name.to_owned()))
        }
        JsResultAction::Delete => {
            Permission::JsResult(JsResultPermission::Delete(worker_name.to_owned()))
        }
    }
}

pub async fn ensure_js_result_permission(
    token: &str,
    worker_name: &str,
    action: JsResultAction,
) -> anyhow::Result<()> {
    trace!(target: "js_result", worker_name = %worker_name, action = ?action, "checking js_result permission");
    let checker = get_token_checker();
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::JsWorker(worker_name.to_owned())],
            vec![build_required_permission(action, worker_name)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_result", worker_name = %worker_name, action = ?action, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for js_result on worker '{worker_name}', action: {action:?}"
    ))
    .into())
}

pub async fn resolve_accessible_js_result_workers(
    token: &str,
    action: JsResultAction,
) -> anyhow::Result<Vec<String>> {
    trace!(target: "js_result", action = ?action, "resolving accessible js_result workers");
    let checker = get_token_checker();
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let db = ng_db::get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

    let mut worker_names: Vec<String> = js_result::Entity::find()
        .select_only()
        .column(js_result::Column::JsWorkerName)
        .order_by_asc(js_result::Column::JsWorkerName)
        .into_tuple()
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    worker_names.dedup();

    let mut allowed = Vec::new();
    for worker_name in worker_names {
        let is_allowed = checker
            .check_token_limit(
                &token_or_auth,
                vec![Scope::JsWorker(worker_name.clone())],
                vec![build_required_permission(action, worker_name.as_str())],
            )
            .await?;

        if is_allowed {
            allowed.push(worker_name);
        }
    }

    Ok(allowed)
}
