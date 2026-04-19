use crate::token::get::check_token_limit;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Permission, Scope, Terminal};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use tracing::trace;
use uuid::Uuid;

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

    // 同时检查 AgentUuid scope 和 Global scope
    let scopes = vec![Scope::AgentUuid(agent_uuid), Scope::Global];
    let permissions = vec![Permission::Terminal(Terminal::Connect)];

    let has_permission = check_token_limit(&token_or_auth, scopes, permissions).await?;

    if has_permission {
        return Ok(());
    }

    Err(NodegetError::PermissionDenied(format!(
        "No terminal connect permission for agent '{agent_uuid}'"
    ))
    .into())
}
