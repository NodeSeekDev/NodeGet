//! KV 权限校验。
//!
//! 提供 KV 存储的 RBAC 权限校验功能：
//! - Key 校验（`validate_key`、`validate_key_pattern`）
//! - 读写删权限检查（`check_kv_read_permission`、`check_kv_write_permission`、`check_kv_delete_permission`）
//! - 命名空间级权限（`check_kv_delete_namespace_permission`、`check_kv_list_keys_permission`、`resolve_kv_list_namespace_permission`、`check_kv_create_permission`）
//!
//! 权限校验委托至全局 `ng_core::permission::permission_checker::PermissionChecker`。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Kv, Limit, Permission, Scope, Token};
use ng_core::permission::permission_checker::require_permission_checker as get_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use std::collections::HashSet;
use tracing::{debug, trace, warn};

// ── KV 权限类型 ────────────────────────────────────────────────

/// 命名空间列表权限范围
///
/// - `All` — 可列出所有命名空间（SuperToken 或 Global scope）
/// - `Scoped` — 仅可列出指定集合中的命名空间
pub enum KvNamespaceListPermission {
    /// 可列出所有命名空间
    All,
    /// 仅可列出指定集合中的命名空间
    Scoped(HashSet<String>),
}

/// 检查 key 是否包含非法字符（如 *）
///
/// # 参数
/// * `key` - 要检查的 key
///
/// # 返回值
/// 如果 key 合法返回 Ok(()，否则返回错误
pub fn validate_key(key: &str) -> anyhow::Result<()> {
    if key.contains('*') {
        warn!(target: "kv", key = %key, "key validation failed: contains '*'");
        return Err(
            NodegetError::InvalidInput("Key cannot contain '*' character".to_owned()).into(),
        );
    }
    Ok(())
}

/// 检查 key pattern 是否合法（允许后缀通配符）
///
/// 合法形式：
/// - `abc`
/// - `metadata_*`
/// - `*`
pub fn validate_key_pattern(key: &str) -> anyhow::Result<()> {
    if key.is_empty() {
        warn!(target: "kv", "key pattern validation failed: empty key");
        return Err(NodegetError::InvalidInput("Key cannot be empty".to_owned()).into());
    }

    if !key.contains('*') {
        return Ok(());
    }

    let star_count = key.chars().filter(|c| *c == '*').count();
    if (star_count != 1) || !key.ends_with('*') {
        warn!(target: "kv", key = %key, "key pattern validation failed: invalid wildcard");
        return Err(NodegetError::InvalidInput(
            "Wildcard key must contain exactly one '*' and it must be at the end".to_owned(),
        )
        .into());
    }

    Ok(())
}

/// 解析 Token 后的 KV 权限校验中间态（与 `check_token_limit` 语义一致）。
///
/// - `Granted` — 超级令牌，调用方应直接返回 `Ok(())`
/// - `Denied` — 时间无效（未生效 / 已过期），调用方应返回权限拒绝
/// - `Token(token)` — 非超级令牌且时间有效，调用方可用 `token.token_limit` 做内存匹配
///
/// 这样每个 KV 权限函数只需一次 `get_token` + 内存 `check_limits_cover`，
/// 替代原先两次 `check_token_limit` 的全量认证。
enum KvTokenState {
    /// 超级令牌：直接放行
    Granted,
    /// 时间无效：拒绝
    Denied,
    /// 普通令牌且时间有效，可做内存匹配
    Token(Token),
}

/// 解析 Token 并做超级令牌 / 时间有效性判断。
///
/// 错误：认证失败（如 token 不存在）。
async fn resolve_token_for_kv_check(token_or_auth: &TokenOrAuth) -> anyhow::Result<KvTokenState> {
    // 超级令牌直接放行
    let is_super_token = ng_token::check_super_token(token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
    if is_super_token {
        debug!(target: "kv", "super token authenticated, all permissions granted");
        return Ok(KvTokenState::Granted);
    }

    let token = ng_token::get_token(token_or_auth).await?;

    // 检查 Token 有效期（与 check_token_limit 一致）
    let now = get_local_timestamp_ms_i64()?;
    if let Some(from) = token.timestamp_from
        && now < from
    {
        warn!(target: "auth", token_key = %token.token_key, "token not yet valid (timestamp_from)");
        return Ok(KvTokenState::Denied);
    }
    if let Some(to) = token.timestamp_to
        && now > to
    {
        warn!(target: "auth", token_key = %token.token_key, "token expired (timestamp_to)");
        return Ok(KvTokenState::Denied);
    }

    Ok(KvTokenState::Token(token))
}

/// 在已解析的 token_limit 上做全局 OR 具体 key 的内存匹配。
fn limits_cover_global_or_specific(
    limits: &[Limit],
    scope: &Scope,
    global_perm: Permission,
    specific_perm: Permission,
) -> bool {
    ng_token::get::check_limits_cover(limits, scope, &global_perm)
        || ng_token::get::check_limits_cover(limits, scope, &specific_perm)
}

/// 检查是否有 KV 读权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `namespace` - 命名空间
/// * `key` - 要读取的 key
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_read_permission(
    token: &str,
    namespace: &str,
    key: &str,
) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, key = %key, "checking read permission");
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 一次取 token + 内存匹配全局 OR 具体 key（替代两次 check_token_limit 全量认证）
    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, key = %key, "read permission denied");
            return Err(NodegetError::PermissionDenied(format!(
                "No read permission for key '{key}' in namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    let scope = Scope::KvNamespace(namespace.to_owned());
    let covered = limits_cover_global_or_specific(
        &token_info.token_limit,
        &scope,
        Permission::Kv(Kv::Read("*".to_owned())),
        Permission::Kv(Kv::Read(key.to_owned())),
    );

    if covered {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, key = %key, "read permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No read permission for key '{key}' in namespace '{namespace}'"
    ))
    .into())
}

/// 检查是否有 KV 读权限（允许后缀 `*` 通配符）
///
/// # 参数
/// * `token` - 令牌字符串
/// * `namespace` - 命名空间
/// * `key_pattern` - 要读取的 key 或 key 前缀通配符（如 `metadata_*`）
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_read_permission_with_pattern(
    token: &str,
    namespace: &str,
    key_pattern: &str,
) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, key_pattern = %key_pattern, "checking read permission with pattern");
    validate_key_pattern(key_pattern)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, key_pattern = %key_pattern, "read permission denied for pattern");
            return Err(NodegetError::PermissionDenied(format!(
                "No read permission for key '{key_pattern}' in namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    let scope = Scope::KvNamespace(namespace.to_owned());
    let covered = limits_cover_global_or_specific(
        &token_info.token_limit,
        &scope,
        Permission::Kv(Kv::Read("*".to_owned())),
        Permission::Kv(Kv::Read(key_pattern.to_owned())),
    );

    if covered {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, key_pattern = %key_pattern, "read permission denied for pattern");
    Err(NodegetError::PermissionDenied(format!(
        "No read permission for key '{key_pattern}' in namespace '{namespace}'"
    ))
    .into())
}

/// 检查是否有 KV 写权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `namespace` - 命名空间
/// * `key` - 要写入的 key
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_write_permission(
    token: &str,
    namespace: &str,
    key: &str,
) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, key = %key, "checking write permission");
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, key = %key, "write permission denied");
            return Err(NodegetError::PermissionDenied(format!(
                "No write permission for key '{key}' in namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    let scope = Scope::KvNamespace(namespace.to_owned());
    let covered = limits_cover_global_or_specific(
        &token_info.token_limit,
        &scope,
        Permission::Kv(Kv::Write("*".to_owned())),
        Permission::Kv(Kv::Write(key.to_owned())),
    );

    if covered {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, key = %key, "write permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No write permission for key '{key}' in namespace '{namespace}'"
    ))
    .into())
}

/// 检查是否有 KV 删除权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `namespace` - 命名空间
/// * `key` - 要删除的 key
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_delete_permission(
    token: &str,
    namespace: &str,
    key: &str,
) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, key = %key, "checking delete permission");
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, key = %key, "delete permission denied");
            return Err(NodegetError::PermissionDenied(format!(
                "No delete permission for key '{key}' in namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    let scope = Scope::KvNamespace(namespace.to_owned());
    let covered = limits_cover_global_or_specific(
        &token_info.token_limit,
        &scope,
        Permission::Kv(Kv::Delete("*".to_owned())),
        Permission::Kv(Kv::Delete(key.to_owned())),
    );

    if covered {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, key = %key, "delete permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No delete permission for key '{key}' in namespace '{namespace}'"
    ))
    .into())
}

/// 检查是否有删除整个命名空间的权限
///
/// 需要对该命名空间拥有全局删除权限 (`Kv::Delete`("*"))
pub async fn check_kv_delete_namespace_permission(
    token: &str,
    namespace: &str,
) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, "checking delete namespace permission");

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, "delete namespace permission denied");
            return Err(NodegetError::PermissionDenied(format!(
                "No permission to delete namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    // 删除整个命名空间需对该命名空间拥有全局删除权限 (`Kv::Delete`("*"))
    let scope = Scope::KvNamespace(namespace.to_owned());
    let covered = ng_token::get::check_limits_cover(
        &token_info.token_limit,
        &scope,
        &Permission::Kv(Kv::Delete("*".to_owned())),
    );

    if covered {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, "delete namespace permission denied");
    Err(
        NodegetError::PermissionDenied(format!("No permission to delete namespace '{namespace}'"))
            .into(),
    )
}

/// 检查是否有列出所有 keys 的权限
///
/// # 参数
/// * `token` - 令牌字符串
/// * `namespace` - 命名空间
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_list_keys_permission(token: &str, namespace: &str) -> anyhow::Result<()> {
    trace!(target: "kv", namespace = %namespace, "checking list keys permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let token_info = match resolve_token_for_kv_check(&token_or_auth).await? {
        KvTokenState::Granted => return Ok(()),
        KvTokenState::Denied => {
            warn!(target: "kv", namespace = %namespace, "list keys permission denied");
            return Err(NodegetError::PermissionDenied(format!(
                "No permission to list keys in namespace '{namespace}'"
            ))
            .into());
        }
        KvTokenState::Token(t) => t,
    };

    // 检查 ListAllKeys 权限
    let scope = Scope::KvNamespace(namespace.to_owned());
    let has_list_permission = ng_token::get::check_limits_cover(
        &token_info.token_limit,
        &scope,
        &Permission::Kv(Kv::ListAllKeys),
    );

    if has_list_permission {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, "list keys permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "No permission to list keys in namespace '{namespace}'"
    ))
    .into())
}

/// 解析列出 KV 命名空间的权限范围
///
/// 规则：
/// - `Kv::ListAllNamespace` + `Scope::Global` => 可列出所有命名空间
/// - `Kv::ListAllNamespace` + `Scope::KvNamespace(xxx)` => 仅可列出这些命名空间
/// - 其他情况 => 无权限
pub async fn resolve_kv_list_namespace_permission(
    token: &str,
) -> anyhow::Result<KvNamespaceListPermission> {
    trace!(target: "kv", "checking list namespace permission");
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 与其他权限校验保持一致：SuperToken 直接放行
    let is_super_token = checker
        .check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
    if is_super_token {
        debug!(target: "kv", "resolved list namespace permission to All (super token)");
        return Ok(KvNamespaceListPermission::All);
    }

    let token_info = checker.get_token(&token_or_auth).await?;

    // 与 check_token_limit 保持一致：检查 Token 有效期
    let now = get_local_timestamp_ms_i64()?;
    if let Some(from) = token_info.timestamp_from
        && now < from
    {
        return Err(NodegetError::PermissionDenied(
            "Token is not yet valid for listing KV namespaces".to_owned(),
        )
        .into());
    }
    if let Some(to) = token_info.timestamp_to
        && now > to
    {
        return Err(NodegetError::PermissionDenied(
            "Token has expired for listing KV namespaces".to_owned(),
        )
        .into());
    }

    let mut allowed_namespaces = HashSet::new();

    for limit in token_info.token_limit.iter() {
        let has_list_namespace_permission = limit
            .permissions
            .iter()
            .any(|perm| matches!(perm, Permission::Kv(Kv::ListAllNamespace)));

        if !has_list_namespace_permission {
            continue;
        }

        for scope in &limit.scopes {
            match scope {
                Scope::Global => {
                    debug!(target: "kv", "resolved list namespace permission to All (global scope)");
                    return Ok(KvNamespaceListPermission::All);
                }
                Scope::KvNamespace(namespace) => {
                    allowed_namespaces.insert(namespace.clone());
                }
                Scope::AgentUuid(_)
                | Scope::JsWorker(_)
                | Scope::StaticBucket(_)
                | Scope::Db(_) => {}
            }
        }
    }

    if !allowed_namespaces.is_empty() {
        debug!(target: "kv", count = allowed_namespaces.len(), "resolved list namespace permission to Scoped");
        return Ok(KvNamespaceListPermission::Scoped(allowed_namespaces));
    }

    warn!(target: "kv", "list namespace permission denied");
    Err(NodegetError::PermissionDenied("No permission to list KV namespaces".to_owned()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_key ──────────────────────────────────────────────

    #[test]
    fn validate_key_valid_simple() {
        assert!(validate_key("mykey").is_ok());
    }

    #[test]
    fn validate_key_valid_with_slash() {
        assert!(validate_key("path/to/key").is_ok());
    }

    #[test]
    fn validate_key_valid_with_spaces() {
        assert!(validate_key("my key").is_ok());
    }

    #[test]
    fn validate_key_valid_with_dots() {
        assert!(validate_key("config.json").is_ok());
    }

    #[test]
    fn validate_key_valid_with_dashes() {
        assert!(validate_key("my-key-123").is_ok());
    }

    #[test]
    fn validate_key_valid_empty() {
        // validate_key does not check for empty — only checks '*'
        assert!(validate_key("").is_ok());
    }

    #[test]
    fn validate_key_rejects_asterisk() {
        let result = validate_key("bad*key");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("*")));
    }

    #[test]
    fn validate_key_rejects_prefix_wildcard() {
        assert!(validate_key("*key").is_err());
    }

    #[test]
    fn validate_key_rejects_suffix_wildcard() {
        assert!(validate_key("key*").is_err());
    }

    #[test]
    fn validate_key_rejects_standalone_wildcard() {
        assert!(validate_key("*").is_err());
    }

    #[test]
    fn validate_key_rejects_multiple_asterisks() {
        assert!(validate_key("a*b*c").is_err());
    }

    #[test]
    fn validate_key_valid_unicode() {
        assert!(validate_key("键值").is_ok());
    }

    // ── validate_key_pattern ──────────────────────────────────────

    #[test]
    fn validate_key_pattern_valid_simple() {
        assert!(validate_key_pattern("mykey").is_ok());
    }

    #[test]
    fn validate_key_pattern_valid_suffix_wildcard() {
        assert!(validate_key_pattern("metadata_*").is_ok());
    }

    #[test]
    fn validate_key_pattern_valid_standalone_wildcard() {
        assert!(validate_key_pattern("*").is_ok());
    }

    #[test]
    fn validate_key_pattern_valid_prefix_then_wildcard() {
        assert!(validate_key_pattern("abc_*").is_ok());
    }

    #[test]
    fn validate_key_pattern_rejects_empty() {
        let result = validate_key_pattern("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("empty")));
    }

    #[test]
    fn validate_key_pattern_rejects_prefix_wildcard() {
        let result = validate_key_pattern("*key");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("*")));
    }

    #[test]
    fn validate_key_pattern_rejects_middle_wildcard() {
        let result = validate_key_pattern("a*b");
        assert!(result.is_err());
    }

    #[test]
    fn validate_key_pattern_rejects_multiple_asterisks() {
        let result = validate_key_pattern("a**b");
        assert!(result.is_err());
    }

    #[test]
    fn validate_key_pattern_rejects_double_wildcard_at_end() {
        let result = validate_key_pattern("abc_**");
        assert!(result.is_err());
    }

    #[test]
    fn validate_key_pattern_valid_no_wildcard_complex() {
        assert!(validate_key_pattern("config.json").is_ok());
    }
}

/// 检查是否有创建命名空间的权限
/// 只有 `SuperToken` 才有权限创建命名空间
///
/// # 参数
/// * `token` - 令牌字符串
///
/// # 返回值
/// 如果有权限返回 Ok(()，否则返回错误
pub async fn check_kv_create_permission(token: &str) -> anyhow::Result<()> {
    trace!(target: "kv", "checking create namespace permission");
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 只有 SuperToken 才能创建命名空间
    let is_super_token = checker
        .check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if is_super_token {
        return Ok(());
    }

    warn!(target: "kv", "create namespace permission denied: not a super token");
    Err(NodegetError::PermissionDenied("Only SuperToken can create KV namespace".to_owned()).into())
}
