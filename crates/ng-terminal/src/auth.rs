//! Terminal 权限校验模块。
//!
//! 职责：对 Terminal WebSocket 连接进行 Scope + Permission 级别的鉴权。
//!
//! 协作关系：[`check_terminal_connect_permission`] 在 User 连接 WebSocket 前被调用，
//! 权限校验委托至全局 `ng_core::permission::permission_checker::PermissionChecker`。
//!
//! 权限模型：先检查 `AgentUuid` Scope 的 `Terminal::Connect` 权限，
//! 若不通过再回退检查 `Global` Scope，两者任一通过即可连接。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, Terminal};
use ng_core::permission::permission_checker::require_permission_checker as get_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use tracing::{debug, trace, warn};
use uuid::Uuid;

// ── Terminal 连接权限检查 ─────────────────────────────────────────────

/// 校验指定 Token 是否拥有连接到目标 Agent Terminal 的权限。
///
/// - `token` - 完整的 Token 字符串（key:secret 或 username|password 格式）
/// - `agent_uuid` - 目标 Agent 的 UUID 字符串
///
/// 返回：权限通过返回 `Ok(())`，否则返回 `PermissionDenied` 错误。
///
/// 内部步骤：
/// 1. 解析 agent_uuid 为 [`Uuid`]，格式不合法时返回 ParseError
/// 2. 将 token 解析为 [`TokenOrAuth`]
/// 3. 先检查 `AgentUuid` Scope 下的 `Terminal::Connect` 权限
/// 4. 若不通过，回退检查 `Global` Scope 下的 `Terminal::Connect` 权限
/// 5. 两者均不通过时返回 PermissionDenied 错误
pub async fn check_terminal_connect_permission(
    token: &str,
    agent_uuid: &str,
) -> anyhow::Result<()> {
    trace!(target: "terminal", agent_uuid = %agent_uuid, "checking terminal connect permission");
    let agent_uuid = Uuid::parse_str(agent_uuid)
        .map_err(|_| NodegetError::ParseError("Invalid Agent UUID format".to_owned()))?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let checker = get_checker()?;

    // 先检查 AgentUuid Scope 的权限
    let has_agent_permission = checker
        .check_token_limit(
            &token_or_auth,
            &[Scope::AgentUuid(agent_uuid)],
            &[Permission::Terminal(Terminal::Connect)],
        )
        .await?;

    if has_agent_permission {
        debug!(target: "terminal", agent_uuid = %agent_uuid, "AgentUuid Scope 权限通过");
        return Ok(());
    }

    // 回退检查 Global Scope 的权限
    let has_global_permission = checker
        .check_token_limit(
            &token_or_auth,
            &[Scope::Global],
            &[Permission::Terminal(Terminal::Connect)],
        )
        .await?;

    if has_global_permission {
        debug!(target: "terminal", agent_uuid = %agent_uuid, "Global Scope 权限通过");
        return Ok(());
    }

    warn!(target: "terminal", "权限拒绝: 无 Terminal 连接权限, agent_uuid={agent_uuid}");
    Err(NodegetError::PermissionDenied(format!(
        "No terminal connect permission for agent '{agent_uuid}'"
    ))
    .into())
}
