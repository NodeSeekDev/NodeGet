//! Token 查询与权限校验。
//!
//! 核心职责：
//! - `get_token`：根据凭据认证并返回 Token 信息
//! - `get_token_by_key_or_username`：无认证查询（仅超级令牌可用）
//! - `check_token_limit`：检查 Token 是否具备指定 Scope + Permission
//! - `parse_token_limit_with_compat`：兼容性解析 token_limit JSON
//!
//! 权限匹配支持后缀通配符（如 `tcp*` 匹配 `tcp_ping`）。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    CrontabResult, JsResult, Kv, Limit, Permission, Scope, Task, Token,
};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::hash_to_bytes;
use crate::super_token::check_super_token;

/// 认证失败时的统一错误消息，避免泄露 key/username 是否存在等敏感信息。
const AUTH_FAILED_MESSAGE: &str = "Invalid credentials";

/// 根据凭据认证并返回 Token 信息。
///
/// 支持 `key:secret` 和 `username|password` 两种认证方式，
/// 所有哈希比较均使用常量时间（`ct_eq`），防止时序攻击。
///
/// - `token_or_auth`：认证凭据
/// - 返回：认证成功后的 Token 结构体
/// - 错误：凭据无效或未找到对应记录
///
/// 内部步骤：
/// 1. 根据 Token/Auth 分支在缓存中查找对应条目
/// 2. 常量时间比较哈希值验证凭据
/// 3. 从 CachedToken 构建返回用的 Token 结构体（不含哈希）
pub async fn get_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<Token> {
    let cache = TokenCache::global().ok_or_else(|| {
        NodegetError::ConfigNotFound("TokenCache not initialized".to_owned())
    })?;

    let cached_token = match token_or_auth {
        TokenOrAuth::Token(key, secret) => {
            let entry = cache.find_by_key(key).ok_or_else(|| {
                warn!(target: "auth", token_key = %key, "auth failed: token key not found");
                NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned())
            })?;

            let computed = hash_to_bytes(secret);
            if !bool::from(computed.ct_eq(&entry.token_hash_bytes)) {
                warn!(target: "auth", token_key = %key, "auth failed: invalid token secret");
                return Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into());
            }

            debug!(target: "auth", token_key = %key, "token secret verified successfully");
            entry
        }
        TokenOrAuth::Auth(username, password) => {
            let entry = cache.find_by_username(username).ok_or_else(|| {
                warn!(target: "auth", username = %username, "auth failed: username not found");
                NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned())
            })?;

            let computed = hash_to_bytes(password);
            let Some(stored) = &entry.password_hash_bytes else {
                warn!(target: "auth", username = %username, "auth failed: no password set for this user");
                return Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into());
            };
            if !bool::from(computed.ct_eq(stored)) {
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

/// 按 token_key 或 username 查找 Token（无需认证）。
///
/// 此函数用于超级令牌管理模式下的 Token 查询，
/// 不验证 secret/password，仅返回 Token 元信息。
///
/// - `identifier`：token_key 或 username
/// - 返回：匹配的 Token 结构体
/// - 错误：未找到对应记录
///
/// 内部步骤：
/// 1. 优先按 token_key 在缓存中查找
/// 2. 若未找到，回退按 username 查找
pub async fn get_token_by_key_or_username(identifier: &str) -> anyhow::Result<Token> {
    let cache = TokenCache::global().ok_or_else(|| {
        NodegetError::ConfigNotFound("TokenCache not initialized".to_owned())
    })?;

    let cached_token = if let Some(entry) = cache.find_by_key(identifier) {
        debug!(target: "auth", identifier = %identifier, "found token by key");
        entry
    } else {
        cache.find_by_username(identifier).ok_or_else(|| {
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

/// 从 token_limit JSON 中移除无法识别的 Permission 变体。
///
/// 用于向前兼容：当数据库中存储了新版本才有的 Permission 类型时，
/// 旧版本可以先丢弃未知权限再解析，而非直接报错。
///
/// - `token_limit_value`：原始 JSON Value
/// - 返回：过滤后的 JSON Value
fn drop_unknown_permissions(mut token_limit_value: Value) -> Value {
    let Some(limits) = token_limit_value.as_array_mut() else {
        return token_limit_value;
    };

    for limit in limits.iter_mut() {
        let Some(perms) = limit.get_mut("permissions").and_then(Value::as_array_mut) else {
            continue;
        };

        // 仅保留能成功反序列化为 Permission 的条目
        perms.retain(|perm| serde_json::from_value::<Permission>(perm.clone()).is_ok());
    }

    token_limit_value
}

/// 兼容性解析 token_limit JSON 为 Limit 列表。
///
/// 先尝试直接解析；若失败（可能包含未知 Permission 变体），
/// 调用 `drop_unknown_permissions` 过滤后再重试。
///
/// - `token_limit_value`：token_limit 字段的 JSON Value
/// - 返回：解析成功的 Limit 列表
/// - 错误：两次解析均失败
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

/// 通配符匹配函数——仅支持后缀通配符 `*`。
///
/// - `pattern` 以 `*` 结尾时，匹配以 `*` 前内容开头的任意字符串
/// - `pattern` 不以 `*` 结尾时，进行精确匹配
///
/// - `value`：待匹配的值（如具体的 key、namespace 等）
/// - `pattern`：模式串（如 `tcp*`、`*`、`exact_name`）
/// - 返回：是否匹配
fn wildcard_matches_pattern(value: &str, pattern: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| value == pattern, |prefix| value.starts_with(prefix))
}

/// 判断已授权的 Permission 是否覆盖所需的 Permission。
///
/// 精确匹配优先；对于含字符串参数的变体（Kv、CrontabResult、JsResult、Task），
/// 支持后缀通配符匹配。不同操作类型（如 Read vs Write）不互通。
///
/// - `granted`：Token 持有的权限
/// - `required`：请求所需的权限
/// - 返回：granted 是否满足 required
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
        (Permission::Task(Task::Create(pattern)), Permission::Task(Task::Create(task_name)))
        | (Permission::Task(Task::Read(pattern)), Permission::Task(Task::Read(task_name)))
        | (Permission::Task(Task::Write(pattern)), Permission::Task(Task::Write(task_name)))
        | (Permission::Task(Task::Delete(pattern)), Permission::Task(Task::Delete(task_name))) => {
            wildcard_matches_pattern(task_name, pattern)
        }
        _ => false,
    }
}

/// 判断限制中的 Scope 是否覆盖请求所需的 Scope。
///
/// 匹配规则：
/// - `Global` 覆盖所有 Scope
/// - 同类型 Scope 内，JsWorker/StaticBucket/Db 支持通配符，AgentUuid/KvNamespace 需精确匹配
/// - 不同类型 Scope 之间不互通
///
/// - `limit_scope`：Token 限制中的 Scope
/// - `req_scope`：请求所需的 Scope
/// - 返回：limit_scope 是否覆盖 req_scope
fn scope_matches(limit_scope: &Scope, req_scope: &Scope) -> bool {
    match (limit_scope, req_scope) {
        (Scope::Global, _) => true,
        (Scope::AgentUuid(limit_id), Scope::AgentUuid(req_id)) => limit_id == req_id,
        (Scope::KvNamespace(limit_ns), Scope::KvNamespace(req_ns)) => limit_ns == req_ns,
        (Scope::JsWorker(limit_name), Scope::JsWorker(req_name)) => {
            wildcard_matches_pattern(req_name, limit_name)
        }
        (Scope::StaticBucket(limit_name), Scope::StaticBucket(req_name)) => {
            wildcard_matches_pattern(req_name, limit_name)
        }
        (Scope::Db(limit_name), Scope::Db(req_name)) => {
            wildcard_matches_pattern(req_name, limit_name)
        }
        (
            Scope::AgentUuid(_)
            | Scope::KvNamespace(_)
            | Scope::JsWorker(_)
            | Scope::StaticBucket(_)
            | Scope::Db(_),
            Scope::Global,
        )
        | (
            Scope::AgentUuid(_),
            Scope::KvNamespace(_) | Scope::JsWorker(_) | Scope::StaticBucket(_) | Scope::Db(_),
        )
        | (
            Scope::KvNamespace(_),
            Scope::AgentUuid(_) | Scope::JsWorker(_) | Scope::StaticBucket(_) | Scope::Db(_),
        )
        | (
            Scope::JsWorker(_),
            Scope::AgentUuid(_) | Scope::KvNamespace(_) | Scope::StaticBucket(_) | Scope::Db(_),
        )
        | (
            Scope::StaticBucket(_),
            Scope::AgentUuid(_) | Scope::KvNamespace(_) | Scope::JsWorker(_) | Scope::Db(_),
        )
        | (
            Scope::Db(_),
            Scope::AgentUuid(_)
            | Scope::KvNamespace(_)
            | Scope::JsWorker(_)
            | Scope::StaticBucket(_),
        ) => false,
    }
}

/// 检查 Token 是否具备指定的 Scope + Permission 组合。
///
/// 超级令牌直接放行；普通令牌需逐一检查每个请求的 Scope/Permission
/// 是否被 Token 的 Limit 列表覆盖，同时验证时间有效性。
///
/// - `token_or_auth`：认证凭据
/// - `scopes`：请求所需的 Scope 列表
/// - `permissions`：请求所需的 Permission 列表
/// - 返回：`true` 表示权限充足，`false` 表示不足
/// - 错误：认证失败
///
/// 内部步骤：
/// 1. 超级令牌直接返回 true
/// 2. 获取 Token 信息，检查时间有效性（timestamp_from / timestamp_to）
/// 3. 对每个 (req_scope, req_perm) 组合，在 Token 的 Limit 列表中查找覆盖
/// 4. 任一组合未被覆盖则返回 false
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

    // ── wildcard_matches_pattern edge cases ──────────────────────────

    #[test]
    fn test_wildcard_matches_pattern_empty_string() {
        assert!(wildcard_matches_pattern("", ""));
        assert!(wildcard_matches_pattern("", "*"));
        assert!(!wildcard_matches_pattern("", "a*"));
        assert!(!wildcard_matches_pattern("a", ""));
    }

    #[test]
    fn test_wildcard_matches_pattern_star_not_at_end() {
        // "a*b" does NOT end with *, so exact match required
        assert!(!wildcard_matches_pattern("axb", "a*b"));
        // Only trailing * triggers prefix matching
        assert!(wildcard_matches_pattern("a*b", "a*b"));
    }

    #[test]
    fn test_wildcard_matches_pattern_prefix_star_empty_prefix() {
        // "*" has prefix "" — matches everything
        assert!(wildcard_matches_pattern("anything", "*"));
        assert!(wildcard_matches_pattern("", "*"));
    }

    #[test]
    fn test_wildcard_matches_pattern_prefix_partial() {
        assert!(wildcard_matches_pattern("abc_def", "abc_*"));
        assert!(!wildcard_matches_pattern("ab_def", "abc_*"));
        assert!(wildcard_matches_pattern("abc_", "abc_*"));
    }

    // ── permission_matches: Kv, CrontabResult, JsResult ──────────────

    #[test]
    fn test_permission_matches_kv_exact() {
        assert!(permission_matches(
            &Permission::Kv(Kv::Read("ns1".to_string())),
            &Permission::Kv(Kv::Read("ns1".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::Kv(Kv::Read("ns1".to_string())),
            &Permission::Kv(Kv::Read("ns2".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_kv_wildcard() {
        assert!(permission_matches(
            &Permission::Kv(Kv::Write("*".to_string())),
            &Permission::Kv(Kv::Write("any_ns".to_string())),
        ));
        assert!(permission_matches(
            &Permission::Kv(Kv::Delete("prefix*".to_string())),
            &Permission::Kv(Kv::Delete("prefix_key".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_kv_mismatched_op() {
        assert!(!permission_matches(
            &Permission::Kv(Kv::Read("*".to_string())),
            &Permission::Kv(Kv::Write("ns1".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::Kv(Kv::Write("ns1".to_string())),
            &Permission::Kv(Kv::Delete("ns1".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_crontab_result_wildcard() {
        assert!(permission_matches(
            &Permission::CrontabResult(CrontabResult::Read("*".to_string())),
            &Permission::CrontabResult(CrontabResult::Read("cron_1".to_string())),
        ));
        assert!(permission_matches(
            &Permission::CrontabResult(CrontabResult::Delete("cron_*".to_string())),
            &Permission::CrontabResult(CrontabResult::Delete("cron_abc".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::CrontabResult(CrontabResult::Read("*".to_string())),
            &Permission::CrontabResult(CrontabResult::Delete("cron_1".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_js_result_wildcard() {
        assert!(permission_matches(
            &Permission::JsResult(JsResult::Read("*".to_string())),
            &Permission::JsResult(JsResult::Read("worker1".to_string())),
        ));
        assert!(permission_matches(
            &Permission::JsResult(JsResult::Delete("w_*".to_string())),
            &Permission::JsResult(JsResult::Delete("w_abc".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::JsResult(JsResult::Read("*".to_string())),
            &Permission::JsResult(JsResult::Delete("worker1".to_string())),
        ));
    }

    #[test]
    fn test_permission_matches_cross_variant_denied() {
        // Different Permission variants never match, even with wildcards
        assert!(!permission_matches(
            &Permission::Kv(Kv::Read("*".to_string())),
            &Permission::Task(Task::Read("ns1".to_string())),
        ));
        assert!(!permission_matches(
            &Permission::CrontabResult(CrontabResult::Read("*".to_string())),
            &Permission::JsResult(JsResult::Read("cron_1".to_string())),
        ));
    }

    // ── scope_matches ────────────────────────────────────────────────

    #[test]
    fn test_scope_matches_global_covers_all() {
        assert!(scope_matches(&Scope::Global, &Scope::Global));
        assert!(scope_matches(&Scope::Global, &Scope::KvNamespace("ns".to_string())));
        assert!(scope_matches(&Scope::Global, &Scope::JsWorker("w".to_string())));
        assert!(scope_matches(&Scope::Global, &Scope::StaticBucket("b".to_string())));
        assert!(scope_matches(&Scope::Global, &Scope::Db("d".to_string())));
    }

    #[test]
    fn test_scope_matches_kv_namespace_exact() {
        assert!(scope_matches(
            &Scope::KvNamespace("ns1".to_string()),
            &Scope::KvNamespace("ns1".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::KvNamespace("ns1".to_string()),
            &Scope::KvNamespace("ns2".to_string()),
        ));
    }

    #[test]
    fn test_scope_matches_js_worker_wildcard() {
        assert!(scope_matches(
            &Scope::JsWorker("*".to_string()),
            &Scope::JsWorker("any_name".to_string()),
        ));
        assert!(scope_matches(
            &Scope::JsWorker("prefix*".to_string()),
            &Scope::JsWorker("prefix_worker".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::JsWorker("prefix*".to_string()),
            &Scope::JsWorker("other_worker".to_string()),
        ));
    }

    #[test]
    fn test_scope_matches_static_bucket_wildcard() {
        assert!(scope_matches(
            &Scope::StaticBucket("*".to_string()),
            &Scope::StaticBucket("mybucket".to_string()),
        ));
    }

    #[test]
    fn test_scope_matches_db_wildcard() {
        assert!(scope_matches(
            &Scope::Db("prod*".to_string()),
            &Scope::Db("prod_db".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::Db("prod*".to_string()),
            &Scope::Db("dev_db".to_string()),
        ));
    }

    #[test]
    fn test_scope_matches_cross_type_denied() {
        assert!(!scope_matches(
            &Scope::KvNamespace("ns".to_string()),
            &Scope::JsWorker("w".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::JsWorker("w".to_string()),
            &Scope::StaticBucket("b".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::StaticBucket("b".to_string()),
            &Scope::Db("d".to_string()),
        ));
        assert!(!scope_matches(
            &Scope::Db("d".to_string()),
            &Scope::KvNamespace("ns".to_string()),
        ));
        // Non-Global scopes do NOT cover Scope::Global
        assert!(!scope_matches(
            &Scope::KvNamespace("ns".to_string()),
            &Scope::Global,
        ));
    }

    // ── drop_unknown_permissions ─────────────────────────────────────

    #[test]
    fn test_drop_unknown_permissions_removes_invalid() {
        let input = serde_json::json!([
            {
                "scopes": [{"global": null}],
                "permissions": [
                    {"task": {"create": "ping"}},
                    {"non_existent_variant": {"create": "ping"}}
                ]
            }
        ]);
        let output = drop_unknown_permissions(input);
        let limits = output.as_array().unwrap();
        let perms = limits[0].get("permissions").unwrap().as_array().unwrap();
        assert_eq!(perms.len(), 1, "unknown permission should be dropped");
        assert!(perms[0].get("task").is_some());
    }

    #[test]
    fn test_drop_unknown_permissions_keeps_all_valid() {
        let input = serde_json::json!([
            {
                "scopes": [{"global": null}],
                "permissions": [
                    {"task": {"create": "ping"}},
                    {"kv": {"read": "ns1"}}
                ]
            }
        ]);
        let output = drop_unknown_permissions(input.clone());
        assert_eq!(output, input, "all-valid input should be unchanged");
    }

    #[test]
    fn test_drop_unknown_permissions_non_array_returns_unchanged() {
        let input = serde_json::json!({"not": "an array"});
        let output = drop_unknown_permissions(input.clone());
        assert_eq!(output, input);
    }

    #[test]
    fn test_drop_unknown_permissions_limit_without_permissions_key() {
        let input = serde_json::json!([
            {"scopes": [{"global": null}]}
        ]);
        let output = drop_unknown_permissions(input.clone());
        assert_eq!(output, input, "limit without permissions key is preserved");
    }

    // ── parse_token_limit_with_compat ────────────────────────────────

    #[test]
    fn test_parse_token_limit_with_compat_valid() {
        let input = serde_json::json!([
            {
                "scopes": [{"global": null}],
                "permissions": [{"task": {"create": "ping"}}]
            }
        ]);
        let limits = parse_token_limit_with_compat(input).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].scopes, vec![Scope::Global]);
    }

    #[test]
    fn test_parse_token_limit_with_compat_filters_unknown_then_succeeds() {
        let input = serde_json::json!([
            {
                "scopes": [{"global": null}],
                "permissions": [
                    {"task": {"create": "ping"}},
                    {"fake_permission": {"create": "ping"}}
                ]
            }
        ]);
        let limits = parse_token_limit_with_compat(input).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].permissions.len(), 1);
    }

    #[test]
    fn test_parse_token_limit_with_compat_all_invalid_permissions_yields_empty_perms() {
        // When all permissions are unrecognized, they get filtered out,
        // but the Limit itself remains valid (with empty permissions).
        let input = serde_json::json!([
            {
                "scopes": [{"global": null}],
                "permissions": [{"fake_permission": null}]
            }
        ]);
        let limits = parse_token_limit_with_compat(input).unwrap();
        assert_eq!(limits.len(), 1);
        assert!(limits[0].permissions.is_empty());
        assert_eq!(limits[0].scopes, vec![Scope::Global]);
    }

    #[test]
    fn test_parse_token_limit_with_compat_completely_invalid_json_fails() {
        // A structurally invalid JSON that can't parse at all
        let input = serde_json::json!("not an array");
        assert!(parse_token_limit_with_compat(input).is_err());
    }

    #[test]
    fn test_parse_token_limit_with_compat_empty_array() {
        let limits = parse_token_limit_with_compat(serde_json::json!([])).unwrap();
        assert!(limits.is_empty());
    }
}
