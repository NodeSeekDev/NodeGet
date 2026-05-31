use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, Terminal};
use ng_core::permission::token_auth::TokenOrAuth;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use tracing::trace;
use uuid::Uuid;

// ── TokenPermissionChecker trait + global injection ────────────────────

/// Trait for token permission checking operations needed by terminal auth.
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

// ── Terminal permission checking ───────────────────────────────────────

/// 检查是否有 Terminal 连接权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `agent_uuid` - Agent 的 UUID
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_terminal_connect_permission(
    token: &str,
    agent_uuid: &str,
) -> anyhow::Result<()> {
    trace!(target: "terminal", agent_uuid = %agent_uuid, "checking terminal connect permission");
    // 解析 Agent UUID
    let agent_uuid = Uuid::parse_str(agent_uuid)
        .map_err(|_| NodegetError::ParseError("Invalid Agent UUID format".to_owned()))?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let checker = get_token_checker();

    // 先检查 AgentUuid scope 的权限
    let has_agent_permission = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::Terminal(Terminal::Connect)],
        )
        .await?;

    if has_agent_permission {
        return Ok(());
    }

    // 也检查 Global scope 的权限
    let has_global_permission = checker
        .check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::Terminal(Terminal::Connect)],
        )
        .await?;

    if has_global_permission {
        return Ok(());
    }

    Err(NodegetError::PermissionDenied(format!(
        "No terminal connect permission for agent '{agent_uuid}'"
    ))
    .into())
}
