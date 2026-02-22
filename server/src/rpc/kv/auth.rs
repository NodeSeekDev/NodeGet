use crate::token::get::check_token_limit;
use crate::token::super_token::check_super_token;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Kv, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;

/// 检查 key 是否包含非法字符（如 *）
///
/// # 参数
/// * `key` - 要检查的 key
///
/// # 返回值
/// 如果 key 合法返回 Ok(()，否则返回错误
pub fn validate_key(key: &str) -> anyhow::Result<()> {
    if key.contains('*') {
        return Err(
            NodegetError::InvalidInput("Key cannot contain '*' character".to_owned()).into(),
        );
    }
    Ok(())
}

/// 检查 key 是否匹配权限模式
///
/// # 参数
/// * `key` - 要检查的 key
/// * `pattern` - 权限模式（可能包含 * 通配符）
///
/// # 返回值
/// 如果 key 匹配模式返回 true
fn key_matches_pattern(key: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| key == pattern, |prefix| key.starts_with(prefix))
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
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局读权限（key 为 "*" 表示所有 key）
    let global_read_perm = Permission::Kv(Kv::Read("*".to_owned()));
    let has_global_read =
        check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_read_perm]).await?;

    if has_global_read {
        return Ok(());
    }

    // 检查是否有特定 key 的读权限
    let specific_read_perm = Permission::Kv(Kv::Read(key.to_owned()));
    let has_specific_read = check_token_limit(
        &token_or_auth,
        vec![scope.clone()],
        vec![specific_read_perm],
    )
    .await?;

    if has_specific_read {
        return Ok(());
    }

    // 检查通配符权限（如 "metadata_*"）
    // 需要获取 token 并手动检查权限
    let token_info = crate::token::get::get_token(&token_or_auth).await?;

    for limit in &token_info.token_limit {
        // 检查 scope 是否匹配
        let scope_matches = limit.scopes.iter().any(|s| match s {
            Scope::Global => true,
            Scope::KvNamespace(ns) => ns == namespace,
            Scope::AgentUuid(_) => false,
        });

        if !scope_matches {
            continue;
        }

        // 检查权限
        for perm in &limit.permissions {
            if let Permission::Kv(Kv::Read(pattern)) = perm
                && key_matches_pattern(key, pattern)
            {
                return Ok(());
            }
        }
    }

    Err(NodegetError::PermissionDenied(format!(
        "No read permission for key '{key}' in namespace '{namespace}'"
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
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局写权限（key 为 "*" 表示所有 key）
    let global_write_perm = Permission::Kv(Kv::Write("*".to_owned()));
    let has_global_write =
        check_token_limit(&token_or_auth, vec![scope.clone()], vec![global_write_perm]).await?;

    if has_global_write {
        return Ok(());
    }

    // 检查是否有特定 key 的写权限
    let specific_write_perm = Permission::Kv(Kv::Write(key.to_owned()));
    let has_specific_write = check_token_limit(
        &token_or_auth,
        vec![scope.clone()],
        vec![specific_write_perm],
    )
    .await?;

    if has_specific_write {
        return Ok(());
    }

    // 检查通配符权限
    let token_info = crate::token::get::get_token(&token_or_auth).await?;

    for limit in &token_info.token_limit {
        let scope_matches = limit.scopes.iter().any(|s| match s {
            Scope::Global => true,
            Scope::KvNamespace(ns) => ns == namespace,
            Scope::AgentUuid(_) => false,
        });

        if !scope_matches {
            continue;
        }

        for perm in &limit.permissions {
            if let Permission::Kv(Kv::Write(pattern)) = perm
                && key_matches_pattern(key, pattern)
            {
                return Ok(());
            }
        }
    }

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
    // 验证 key 不包含非法字符
    validate_key(key)?;

    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 先检查是否有全局删除权限（key 为 "*" 表示所有 key）
    let global_delete_perm = Permission::Kv(Kv::Delete("*".to_owned()));
    let has_global_delete = check_token_limit(
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
    let has_specific_delete = check_token_limit(
        &token_or_auth,
        vec![scope.clone()],
        vec![specific_delete_perm],
    )
    .await?;

    if has_specific_delete {
        return Ok(());
    }

    // 检查通配符权限
    let token_info = crate::token::get::get_token(&token_or_auth).await?;

    for limit in &token_info.token_limit {
        let scope_matches = limit.scopes.iter().any(|s| match s {
            Scope::Global => true,
            Scope::KvNamespace(ns) => ns == namespace,
            Scope::AgentUuid(_) => false,
        });

        if !scope_matches {
            continue;
        }

        for perm in &limit.permissions {
            if let Permission::Kv(Kv::Delete(pattern)) = perm
                && key_matches_pattern(key, pattern)
            {
                return Ok(());
            }
        }
    }

    Err(NodegetError::PermissionDenied(format!(
        "No delete permission for key '{key}' in namespace '{namespace}'"
    ))
    .into())
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
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 构建 scope - 使用 KvNamespace
    let scope = Scope::KvNamespace(namespace.to_owned());

    // 检查 ListAllKeys 权限
    let list_perm = Permission::Kv(Kv::ListAllKeys);
    let has_list_permission =
        check_token_limit(&token_or_auth, vec![scope], vec![list_perm]).await?;

    if has_list_permission {
        return Ok(());
    }

    Err(NodegetError::PermissionDenied(format!(
        "No permission to list keys in namespace '{namespace}'"
    ))
    .into())
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
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 只有 SuperToken 才能创建命名空间
    let is_super_token = check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if is_super_token {
        return Ok(());
    }

    Err(NodegetError::PermissionDenied("Only SuperToken can create KV namespace".to_owned()).into())
}
