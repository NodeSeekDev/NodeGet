//! KV 权限校验。
//!
//! 提供 KV 存储的 RBAC 权限校验功能：
//! - Key 校验（`validate_key`、`validate_key_pattern`）
//! - 读写删权限检查（`check_kv_read_permission`、`check_kv_write_permission`、`check_kv_delete_permission`）
//! - 命名空间级权限（`check_kv_delete_namespace_permission`、`check_kv_list_keys_permission`、`resolve_kv_list_namespace_permission`、`check_kv_create_permission`）
//!
//! 权限校验委托至全局 `ng_core::permission::permission_checker::PermissionChecker`。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Kv, Permission, Scope};
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

    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局读权限（key 为 "*" 表示所有 key）
    let global_read_perm = Permission::Kv(Kv::Read("*".to_owned()));
    let has_global_read = checker
        .check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_read_perm])
        .await?;

    if has_global_read {
        return Ok(());
    }

    // 检查是否有特定 key 的读权限
    let specific_read_perm = Permission::Kv(Kv::Read(key.to_owned()));
    let has_specific_read = checker
        .check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![specific_read_perm],
        )
        .await?;

    if has_specific_read {
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

    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let scope = Scope::KvNamespace(namespace.to_owned());

    let global_read_perm = Permission::Kv(Kv::Read("*".to_owned()));
    let has_global_read = checker
        .check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_read_perm])
        .await?;

    if has_global_read {
        return Ok(());
    }

    let specific_read_perm = Permission::Kv(Kv::Read(key_pattern.to_owned()));
    let has_specific_read = checker
        .check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![specific_read_perm],
        )
        .await?;

    if has_specific_read {
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

    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局写权限（key 为 "*" 表示所有 key）
    let global_write_perm = Permission::Kv(Kv::Write("*".to_owned()));
    let has_global_write = checker
        .check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_write_perm])
        .await?;

    if has_global_write {
        return Ok(());
    }

    // 检查是否有特定 key 的写权限
    let specific_write_perm = Permission::Kv(Kv::Write(key.to_owned()));
    let has_specific_write = checker
        .check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![specific_write_perm],
        )
        .await?;

    if has_specific_write {
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

    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局删除权限（key 为 "*" 表示所有 key）
    let global_delete_perm = Permission::Kv(Kv::Delete("*".to_owned()));
    let has_global_delete = checker
        .check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![global_delete_perm],
        )
        .await?;

    if has_global_delete {
        return Ok(());
    }

    // 检查是否有特定 key 的删除权限
    let specific_delete_perm = Permission::Kv(Kv::Delete(key.to_owned()));
    let has_specific_delete = checker
        .check_token_limit(
            &token_or_auth,
            vec![scope.clone()],
            vec![specific_delete_perm],
        )
        .await?;

    if has_specific_delete {
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

    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let scope = Scope::KvNamespace(namespace.to_owned());
    let global_delete_perm = Permission::Kv(Kv::Delete("*".to_owned()));
    let has_global_delete = checker
        .check_token_limit(&token_or_auth, vec![scope], vec![global_delete_perm])
        .await?;

    if has_global_delete {
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
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 检查 ListAllKeys 权限
    let list_perm = Permission::Kv(Kv::ListAllKeys);
    let has_list_permission = checker
        .check_token_limit(&token_or_auth, vec![scope], vec![list_perm])
        .await?;

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

    for limit in &token_info.token_limit {
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
