//! Token 内存缓存，基于 DB 的全量加载模式。
//!
//! 核心职责：将 token 表全量加载到内存，提供按 key / username / super token 的快速查询。
//! 使用 `DbBackedCache` + `make_global_cache!` 宏生成全局单例。
//!
//! 协作关系：
//! - 被 `get`、`super_token`、`generate_token` 等模块依赖查询
//! - DB 变更后由各 RPC 方法主动调用 `TokenCache::reload()` 同步

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Limit;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::token;
use ng_infra::make_global_cache;
use ng_infra::server::{DbBackedCache, load_from_db};
use subtle::ConstantTimeEq;
use tracing::{debug, warn};

use crate::get::parse_token_limit_with_compat;
use crate::hash_to_bytes;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

/// 认证失败时的统一错误消息，避免泄露具体是 key 还是 secret 不匹配
const AUTH_FAILED_MESSAGE: &str = "Invalid credentials";

/// 缓存中的 Token 条目，包含预计算的哈希值以加速认证。
pub struct CachedToken {
    /// 数据库中的 Token 原始模型
    pub model: Arc<token::Model>,
    /// 预解析的权限限制列表，避免每次认证时重复反序列化
    pub parsed_limits: Vec<Limit>,
    /// token_secret 的 SHA256 原始摘要（32 字节），用于常量时间比较
    pub token_hash_bytes: [u8; 32],
    /// password 的 SHA256 原始摘要，仅用户名/密码认证时使用；无密码则为 None
    pub password_hash_bytes: Option<[u8; 32]>,
}

/// 缓存内部索引结构，按不同维度组织 Token 条目。
struct TokenCacheInner {
    /// 以 token_key 为键的索引，用于 `key:secret` 认证路径
    by_key: HashMap<String, Arc<CachedToken>>,
    /// 以 username 为键的索引，用于 `username|password` 认证路径
    by_username: HashMap<String, Arc<CachedToken>>,
    /// ID 为 1 的超级令牌条目，单独缓存以加速高频鉴权
    super_token: Option<Arc<CachedToken>>,
}

/// 基于 DB 的 Token 内存缓存，使用 RwLock 保护内部索引。
///
/// 提供 `find_by_key`、`find_by_username`、`get_super_token`、`authenticate` 等查询方法，
/// 以及 `DbBackedCache` trait 要求的 `build_cache` / `reload_from_models` / `load_all` 生命周期方法。
pub struct TokenCache {
    inner: RwLock<TokenCacheInner>,
}

/// 从 RwLock 获取读锁，若锁被 poisoned 则恢复而非 panic。
///
/// 生产环境中某线程 panic 导致锁中毒不应使整个服务不可用，
/// 因此选择恢复并继续使用中毒时的数据。
fn recover_read(lock: &RwLock<TokenCacheInner>) -> std::sync::RwLockReadGuard<'_, TokenCacheInner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "token_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

/// 从 RwLock 获取写锁，若锁被 poisoned 则恢复而非 panic。
fn recover_write(
    lock: &RwLock<TokenCacheInner>,
) -> std::sync::RwLockWriteGuard<'_, TokenCacheInner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "token_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

// 使用 make_global_cache! 宏生成全局单例：TOKEN_CACHE_GLOBAL（OnceLock）
// 提供 init() / global() / reload() 方法，遵循 workspace 统一缓存模式
make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);

impl DbBackedCache for TokenCache {
    type Model = token::Model;

    /// 缓存名称标识，用于日志和调试。
    fn cache_name() -> &'static str {
        "token"
    }

    /// 从数据库模型列表构建缓存实例。
    ///
    /// 1. 调用 `build_maps` 生成 by_key / by_username / super_token 三个索引
    /// 2. 包装为 RwLock 保护的内层结构
    fn build_cache(models: Vec<Self::Model>) -> Self {
        let (by_key, by_username, super_token) = Self::build_maps(models);
        Self {
            inner: RwLock::new(TokenCacheInner {
                by_key,
                by_username,
                super_token,
            }),
        }
    }

    /// 用新的模型列表原地刷新缓存内容。
    ///
    /// 与 `build_cache` 不同，此方法复用已有的 `TokenCache` 实例，
    /// 仅替换内部索引数据，避免重建全局单例。
    #[allow(clippy::unused_async)]
    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let (by_key, by_username, super_token) = Self::build_maps(models);
        let mut guard = recover_write(&self.inner);
        let old_by_key = std::mem::replace(&mut guard.by_key, by_key);
        let old_by_username = std::mem::replace(&mut guard.by_username, by_username);
        guard.super_token = super_token;
        drop(guard); // 显式释放写锁，旧 HashMap 在锁外 drop
        drop(old_by_key);
        drop(old_by_username);
    }

    /// 从数据库全量加载所有 Token 记录。
    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<token::Entity>()
    }
}

impl TokenCache {
    /// 从数据库模型列表构建三个索引映射。
    ///
    /// 1. 遍历所有 Token 记录，预解析 token_limit 并将 hex 哈希转为原始字节
    /// 2. ID 为 1 的记录单独存储为 super_token
    /// 3. 有 username 的记录额外插入 by_username 索引
    ///
    /// - `all_tokens`：从数据库加载的全部 Token 模型
    /// - 返回：`(by_key 索引, by_username 索引, super_token 条目)`
    fn build_maps(
        all_tokens: Vec<token::Model>,
    ) -> (
        HashMap<String, Arc<CachedToken>>,
        HashMap<String, Arc<CachedToken>>,
        Option<Arc<CachedToken>>,
    ) {
        let mut by_key = HashMap::with_capacity(all_tokens.len());
        let mut by_username = HashMap::with_capacity(all_tokens.len());
        let mut super_token: Option<Arc<CachedToken>> = None;

        for model in all_tokens {
            // 预解析权限列表；解析失败时记录警告并使用空列表，避免因脏数据导致整个缓存不可用
            let parsed_limits = parse_token_limit_with_compat(model.token_limit.clone())
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        target: "token",
                        token_key = %model.token_key,
                        error = %e,
                        "failed to pre-parse token_limit, using empty"
                    );
                    Vec::new()
                });

            // hex → 原始字节；转换失败时填零而非跳过，确保后续常量时间比较不会误匹配
            let token_hash_bytes = hex_to_bytes(&model.token_hash).unwrap_or([0u8; 32]);
            let password_hash_bytes = model.password_hash.as_deref().and_then(hex_to_bytes);

            let cached = Arc::new(CachedToken {
                model: Arc::new(model),
                parsed_limits,
                token_hash_bytes,
                password_hash_bytes,
            });

            // ID 为 1 的记录即超级令牌，单独缓存
            if cached.model.id == 1 {
                super_token = Some(Arc::clone(&cached));
            }
            by_key.insert(cached.model.token_key.clone(), Arc::clone(&cached));
            if let Some(ref uname) = cached.model.username {
                by_username.insert(uname.clone(), cached);
            }
        }

        (by_key, by_username, super_token)
    }

    /// 按 token_key 查找缓存条目。
    ///
    /// - `key`：Token 的 key 部分（不含 secret）
    /// - 返回：匹配的 CachedToken，未找到则为 None
    pub fn find_by_key(&self, key: &str) -> Option<Arc<CachedToken>> {
        recover_read(&self.inner).by_key.get(key).map(Arc::clone)
    }

    /// 按 username 查找缓存条目。
    ///
    /// - `username`：Token 关联的用户名
    /// - 返回：匹配的 CachedToken，未找到则为 None
    pub fn find_by_username(&self, username: &str) -> Option<Arc<CachedToken>> {
        recover_read(&self.inner)
            .by_username
            .get(username)
            .map(Arc::clone)
    }

    /// 获取超级令牌缓存条目。
    ///
    /// - 返回：ID 为 1 的 CachedToken，若不存在则为 None
    pub fn get_super_token(&self) -> Option<Arc<CachedToken>> {
        recover_read(&self.inner)
            .super_token
            .as_ref()
            .map(Arc::clone)
    }

    /// 获取所有缓存条目（不含排序）。
    ///
    /// - 返回：所有 CachedToken 的列表
    pub fn get_all(&self) -> Vec<Arc<CachedToken>> {
        recover_read(&self.inner)
            .by_key
            .values()
            .map(Arc::clone)
            .collect()
    }

    /// 在单次锁获取内完成认证，避免多次加锁的竞争窗口。
    ///
    /// 优先检查超级令牌，再回退到普通令牌查询。
    /// 所有比较均使用常量时间（`ct_eq`），防止时序攻击。
    ///
    /// - `token_or_auth`：认证凭据，支持 `Token(key, secret)` 或 `Auth(username, password)`
    /// - 返回：成功时为 `(CachedToken, 是否为超级令牌)`；失败时返回错误
    /// - 错误：认证失败或缓存中缺少超级令牌记录
    ///
    /// 内部步骤（Token 认证路径）：
    /// 1. 常量时间比较 key 与超级令牌的 token_key
    /// 2. 若 key 匹配，常量时间比较 secret 哈希与超级令牌的 token_hash_bytes
    /// 3. 若超级令牌不匹配，在 by_key 索引中查找普通令牌
    ///
    /// 内部步骤（Basic Auth 路径）：
    /// 1. 常量时间比较 username 与超级令牌的 username
    /// 2. 若匹配，常量时间比较 password 哈希
    /// 3. 若超级令牌不匹配，在 by_username 索引中查找普通令牌
    pub fn authenticate(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> anyhow::Result<(Arc<CachedToken>, bool)> {
        let inner = recover_read(&self.inner);

        // 确保超级令牌存在（与 check_super_token 行为保持一致）
        let super_entry = inner.super_token.as_ref().ok_or_else(|| {
            NodegetError::NotFound("Super Token record (ID 1) not found in cache".to_owned())
        })?;

        match token_or_auth {
            TokenOrAuth::Token(key, secret) => {
                // 优先检查超级令牌
                let key_match: bool = key
                    .as_bytes()
                    .ct_eq(super_entry.model.token_key.as_bytes())
                    .into();
                if key_match {
                    let computed = hash_to_bytes(secret);
                    let hash_match: bool = computed.ct_eq(&super_entry.token_hash_bytes).into();
                    debug!(target: "auth", is_super = hash_match, "super token check (token auth)");
                    if hash_match {
                        return Ok((Arc::clone(super_entry), true));
                    }
                    // key 匹配超级令牌但 secret 不匹配，继续检查普通令牌
                }

                // 在 by_key 索引中查找普通令牌
                if let Some(cached) = inner.by_key.get(key) {
                    let computed = hash_to_bytes(secret);
                    if bool::from(computed.ct_eq(&cached.token_hash_bytes)) {
                        debug!(target: "auth", token_key = %key, "token secret verified successfully");
                        return Ok((Arc::clone(cached), false));
                    }
                    warn!(target: "auth", token_key = %key, "auth failed: invalid token secret");
                    return Err(
                        NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into(),
                    );
                }

                warn!(target: "auth", token_key = %key, "auth failed: token key not found");
                Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into())
            }
            TokenOrAuth::Auth(username, password) => {
                // 优先检查超级令牌
                let username_match = super_entry
                    .model
                    .username
                    .as_deref()
                    .is_some_and(|u| u.as_bytes().ct_eq(username.as_bytes()).into());
                if username_match && let Some(stored) = &super_entry.password_hash_bytes {
                    let computed = hash_to_bytes(password);
                    if bool::from(computed.ct_eq(stored)) {
                        debug!(target: "auth", is_super = true, "authenticate: super token (basic auth)");
                        return Ok((Arc::clone(super_entry), true));
                    }
                    debug!(target: "auth", is_super = false, "super token check (basic auth), password mismatch");
                }
                // username 匹配超级令牌但 password 不匹配（或未设置密码），继续检查普通令牌

                // 在 by_username 索引中查找普通令牌
                if let Some(cached) = inner.by_username.get(username) {
                    let computed = hash_to_bytes(password);
                    let Some(stored) = &cached.password_hash_bytes else {
                        warn!(target: "auth", username = %username, "auth failed: no password set for this user");
                        return Err(
                            NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into(),
                        );
                    };
                    if bool::from(computed.ct_eq(stored)) {
                        debug!(target: "auth", username = %username, "password verified successfully");
                        return Ok((Arc::clone(cached), false));
                    }
                    warn!(target: "auth", username = %username, "auth failed: invalid password");
                    return Err(
                        NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into(),
                    );
                }

                warn!(target: "auth", username = %username, "auth failed: username not found");
                Err(NodegetError::PermissionDenied(AUTH_FAILED_MESSAGE.to_owned()).into())
            }
        }
    }
}

/// 将 64 字符的十六进制字符串转换为 32 字节原始数组。
///
/// 手动解析而非使用 `hex::decode`，避免堆分配和 Vec 转换的开销。
/// 用于将数据库中存储的 hex 哈希转为常量时间比较所需的原始字节。
///
/// - `hex_str`：64 字符的十六进制字符串
/// - 返回：32 字节数组，长度不合法或含非法字符时返回 None
fn hex_to_bytes(hex_str: &str) -> Option<[u8; 32]> {
    if hex_str.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        let hi = hex_str.as_bytes().get(i * 2)?;
        let lo = hex_str.as_bytes().get(i * 2 + 1)?;
        bytes[i] = (hex_nibble(*hi)? << 4) | hex_nibble(*lo)?;
    }
    Some(bytes)
}

/// 将单个 ASCII 十六进制字符转换为数值。
///
/// - `b`：ASCII 字节（`0-9`、`a-f`、`A-F`）
/// - 返回：对应的 0-15 数值，非法字符返回 None
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
