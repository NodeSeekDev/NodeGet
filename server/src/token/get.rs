use crate::token::cache::TokenCache;
use crate::token::hash_string;
use crate::token::super_token::check_super_token;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{
    CrontabResult, JsResult, Kv, Limit, Permission, Scope, Task, Token,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tracing::{debug, warn};

/// 统一的身份验证失败错误消息，防止信息泄露
const AUTH_FAILED_MESSAGE: &str = "Invalid credentials";

/// 使用恒定时间比较验证哈希，防止时序攻击
fn verify_hash_constant_time(computed_hash: &str, stored_hash: &str) -> bool {
    computed_hash
        .as_bytes()
        .ct_eq(stored_hash.as_bytes())
        .into()
}

pub async fn get_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<Token> {
    let cache = TokenCache::global();

    let cached_token = match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            let entry = cache.find_by_key(key).await.ok_or_else(|| {
                warn!(target: "auth", token_key = %key, "auth failed: token key not found");
                NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned())
            })?;

            let computed_hash = hash_string(secret);
            if !verify_hash_constant_time(&computed_hash, &entry.model.token_hash) {
                warn!(target: "auth", token_key = %key, "auth failed: invalid token secret");
                return Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into());
            }

            debug!(target: "auth", token_key = %key, "token secret verified successfully");
            entry
        }
        TokenOrAuth::Auth(username, password) => {
            let entry = cache.find_by_username(username).await.ok_or_else(|| {
                warn!(target: "auth", username = %username, "auth failed: username not found");
                NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned())
            })?;

            let p_hash = hash_string(password);
            let stored_hash = entry.model.password_hash.as_deref().unwrap_or("");
            if !verify_hash_constant_time(&p_hash, stored_hash) {
                warn!(target: "auth", username = %username, "auth failed: invalid password");
                return Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into());
            }

            debug!(target: "auth", username = %username, "password verified successfully");
            entry
        }
    };

    debug!(target: "auth", token_key = %cached_token.model.token_key, limits_count = cached_token.parsed_limits.len(), "token authenticated successfully");

    Ok(Token {
        version: cached_token.model.version,
        token_key: cached_token.model.token_key.clone(),
        timestamp_from: cached_token.model.time_stamp_from,
        timestamp_to: cached_token.model.time_stamp_to,
        token_limit: cached_token.parsed_limits.clone(),
        username: cached_token.model.username.clone(),
    })
}

pub async fn get_token_by_key_or_username(identifier: &str) -> anyhow::Result<Token> {
    let cache = TokenCache::global();

    let cached_token = if let Some(entry) = cache.find_by_key(identifier).await {
        debug!(target: "auth", identifier = %identifier, "found token by key");
        entry
    } else {
        cache.find_by_username(identifier).await.ok_or_else(|| {
            warn!(target: "auth", identifier = %identifier, "token not found by key or username");
            NodegetError::NotFound(format!("Token not found by key/username: {identifier}"))
        })?
    };

    debug!(target: "auth", identifier = %identifier, token_key = %cached_token.model.token_key, "token resolved successfully");

    Ok(Token {
        version: cached_token.model.version,
        token_key: cached_token.model.token_key.clone(),
        timestamp_from: cached_token.model.time_stamp_from,
        timestamp_to: cached_token.model.time_stamp_to,
        token_limit: cached_token.parsed_limits.clone(),
        username: cached_token.model.username.clone(),
    })
}

fn drop_unknown_permissions(mut token_limit_value: Value) -> Value {
    let Some(limits) = token_limit_value.as_array_mut() else {
        return token_limit_value;
    };

    for limit in limits.iter_mut() {
        let Some(perms) = limit.get_mut("permissions").and_then(Value::as_array_mut) else {
            continue;
        };

        perms.retain(|perm| serde_json::from_value::<Permission>(perm.clone()).is_ok());
    }

    token_limit_value
}

pub fn parse_token_limit_with_compat(token_limit_value: Value) -> anyhow::Result<Vec<Limit>> {
    match serde_json::from_value::<Vec<Limit>>(token_limit_value.clone()) {
        Ok(v) => Ok(v),
        Err(original_err) => {
            warn!(target: "auth", error = %original_err, "token_limit parse failed, trying with unknown permissions filtered");
            let filtered = drop_unknown_permissions(token_limit_value);
            serde_json::from_value::<Vec<Limit>>(filtered).map_err(|e| {
                NodegetError::SerializationError(format!(
                    "Failed to parse token permissions: {e}; original error: {original_err}"
                ))
                .into()
            })
        }
    }
}

/// 通配符匹配函数 - 仅支持后缀通配符 `*`
///
/// # 说明
/// - `pattern` 以 `*` 结尾时，匹配以 `*` 前内容开头的任意字符串
/// - `pattern` 不以 `*` 结尾时，进行精确匹配
///
/// # 示例
/// - `wildcard_matches_pattern("abc", "ab*")` -> true
/// - `wildcard_matches_pattern("abc", "abc")` -> true  
/// - `wildcard_matches_pattern("abc", "a*")` -> true
/// - `wildcard_matches_pattern("abc", "xyz")` -> false
fn wildcard_matches_pattern(value: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| value == pattern, |prefix| value.starts_with(prefix))
}

fn permission_matches(granted: &Permission, required: &Permission) -> bool {
    if granted == required {
        return true;
    }

    match (granted, required) {
        (Permission::Kv(Kv::Read(pattern)), Permission::Kv(Kv::Read(key)))
        | (Permission::Kv(Kv::Write(pattern)), Permission::Kv(Kv::Write(key)))
        | (Permission::Kv(Kv::Delete(pattern)), Permission::Kv(Kv::Delete(key))) => {
            wildcard_matches_pattern(key, pattern)
        }
        (
            Permission::CrontabResult(CrontabResult::Read(pattern)),
            Permission::CrontabResult(CrontabResult::Read(cron_name)),
        )
        | (
            Permission::CrontabResult(CrontabResult::Delete(pattern)),
            Permission::CrontabResult(CrontabResult::Delete(cron_name)),
        ) => wildcard_matches_pattern(cron_name, pattern),
        (
            Permission::JsResult(JsResult::Read(pattern)),
            Permission::JsResult(JsResult::Read(worker_name)),
        )
        | (
            Permission::JsResult(JsResult::Delete(pattern)),
            Permission::JsResult(JsResult::Delete(worker_name)),
        ) => wildcard_matches_pattern(worker_name, pattern),
        (
            Permission::Task(Task::Create(pattern)),
            Permission::Task(Task::Create(task_name)),
        )
        | (
            Permission::Task(Task::Read(pattern)),
            Permission::Task(Task::Read(task_name)),
        )
        | (
            Permission::Task(Task::Write(pattern)),
            Permission::Task(Task::Write(task_name)),
        )
        | (
            Permission::Task(Task::Delete(pattern)),
            Permission::Task(Task::Delete(task_name)),
        ) => wildcard_matches_pattern(task_name, pattern),
        _ => false,
    }
}

fn scope_matches(limit_scope: &Scope, req_scope: &Scope) -> bool {
    match (limit_scope, req_scope) {
        (Scope::Global, _) => true,
        (Scope::AgentUuid(limit_id), Scope::AgentUuid(req_id)) => limit_id == req_id,
        (Scope::KvNamespace(limit_ns), Scope::KvNamespace(req_ns)) => limit_ns == req_ns,
        (Scope::JsWorker(limit_name), Scope::JsWorker(req_name)) => {
            wildcard_matches_pattern(req_name, limit_name)
        }
        (Scope::AgentUuid(_) | Scope::KvNamespace(_) | Scope::JsWorker(_), Scope::Global)
        | (Scope::AgentUuid(_), Scope::KvNamespace(_) | Scope::JsWorker(_))
        | (Scope::KvNamespace(_), Scope::AgentUuid(_) | Scope::JsWorker(_))
        | (Scope::JsWorker(_), Scope::AgentUuid(_) | Scope::KvNamespace(_)) => false,
    }
}

pub async fn check_token_limit(
    token_or_auth: &TokenOrAuth,
    scopes: Vec<Scope>,
    permissions: Vec<Permission>,
) -> anyhow::Result<bool> {
    let is_super_token = check_super_token(token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
    if is_super_token {
        debug!(target: "auth", "super token authenticated, all permissions granted");
        return Ok(true);
    }

    let token = get_token(token_or_auth).await?;
    debug!(target: "auth", token_key = %token.token_key, scopes_count = scopes.len(), permissions_count = permissions.len(), "checking token permissions");

    let now = get_local_timestamp_ms_i64()?;
    if let Some(from) = token.timestamp_from
        && now < from
    {
        warn!(target: "auth", token_key = %token.token_key, "token not yet valid (timestamp_from)");
        return Ok(false);
    }
    if let Some(to) = token.timestamp_to
        && now > to
    {
        warn!(target: "auth", token_key = %token.token_key, "token expired (timestamp_to)");
        return Ok(false);
    }

    for req_scope in &scopes {
        for req_perm in &permissions {
            let mut is_allowed = false;

            for limit in &token.token_limit {
                let scope_covered = limit
                    .scopes
                    .iter()
                    .any(|limit_scope| scope_matches(limit_scope, req_scope));
                if !scope_covered {
                    continue;
                }

                if limit
                    .permissions
                    .iter()
                    .any(|perm| permission_matches(perm, req_perm))
                {
                    is_allowed = true;
                    break;
                }
            }

            if !is_allowed {
                warn!(
                    target: "auth",
                    token_key = %token.token_key,
                    scope = ?req_scope,
                    permission = ?req_perm,
                    "permission denied"
                );
                return Ok(false);
            }
        }
    }

    debug!(target: "auth", token_key = %token.token_key, "all permission checks passed");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_matches_pattern_exact() {
        assert!(wildcard_matches_pattern("tcp_ping", "tcp_ping"));
        assert!(wildcard_matches_pattern("ping", "ping"));
        assert!(!wildcard_matches_pattern("tcp_ping", "ping"));
    }

    #[test]
    fn test_wildcard_matches_pattern_star() {
        assert!(wildcard_matches_pattern("tcp_ping", "*"));
        assert!(wildcard_matches_pattern("ping", "*"));
        assert!(wildcard_matches_pattern("http_request", "*"));
    }

    #[test]
    fn test_wildcard_matches_pattern_prefix() {
        assert!(wildcard_matches_pattern("tcp_ping", "tcp*"));
        assert!(wildcard_matches_pattern("tcp_ping", "tc*"));
        assert!(!wildcard_matches_pattern("http_ping", "tcp*"));
        assert!(!wildcard_matches_pattern("ping", "tcp*"));
    }

    #[test]
    fn test_permission_matches_task_exact() {
        assert!(permission_matches(
            &Permission::Task(Task::Write("tcp_ping".to_string())),
            &Permission::Task(Task::Write("tcp_ping".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::Task(Task::Write("tcp_ping".to_string())),
            &Permission::Task(Task::Write("ping".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_task_star() {
        assert!(permission_matches(
            &Permission::Task(Task::Write("*".to_string())),
            &Permission::Task(Task::Write("tcp_ping".to_string())),
        ));
        assert!(permission_matches(
            &Permission::Task(Task::Create("*".to_string())),
            &Permission::Task(Task::Create("ping".to_string())),
        ));
        assert!(permission_matches(
            &Permission::Task(Task::Read("*".to_string())),
            &Permission::Task(Task::Read("http_request".to_string())),
        ));
        assert!(permission_matches(
            &Permission::Task(Task::Delete("*".to_string())),
            &Permission::Task(Task::Delete("web_shell".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_task_prefix() {
        assert!(permission_matches(
            &Permission::Task(Task::Write("tcp*".to_string())),
            &Permission::Task(Task::Write("tcp_ping".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::Task(Task::Write("tcp*".to_string())),
            &Permission::Task(Task::Write("http_ping".to_string())),
        ));
        assert!(permission_matches(
            &Permission::Task(Task::Create("http*".to_string())),
            &Permission::Task(Task::Create("http_request".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_task_mismatched_variant() {
        // Write("*") should NOT match Create("tcp_ping")
        assert!(!permission_matches(
            &Permission::Task(Task::Write("*".to_string())),
            &Permission::Task(Task::Create("tcp_ping".to_string())),
        ));
        // Read("tcp*") should NOT match Write("tcp_ping")
        assert!(!permission_matches(
            &Permission::Task(Task::Read("tcp*".to_string())),
            &Permission::Task(Task::Write("tcp_ping".to_string())),
        ));
    }
}
