//! Server-only infrastructure.
//!
//! This module is only available with the `server` feature.
//! It contains traits and macros that depend on `jsonrpsee`, `sea-orm`,
//! or `serde_json`.
//!
//! ## Contents
//!
//! | Item | Source | Notes |
//! |------|--------|-------|
//! | `AuthChecker` + `set_auth_checker` / `get_auth_checker` | New | Global auth injection |
//! | `load_from_db` | Migrated from `server/src/cache/mod.rs` | Uses `ng_db::get_db()` |
//! | `DbBackedCache` trait | Migrated from `server/src/cache/mod.rs` | Path adjusted |
//! | `make_global_cache!` macro | Migrated from `server/src/cache/mod.rs` | `$crate::server::DbBackedCache` |
//! | `token_identity` | Migrated from `server/src/rpc/mod.rs` | Pure string logic |
//! | `TruncatedRaw` | Migrated from `server/src/rpc/mod.rs` | Uses `serde_json::RawValue` |
//! | `rpc_exec!` macro | Migrated from `server/src/rpc/mod.rs` | `$crate::server::TruncatedRaw` |
//! | `RpcHelper` trait | Migrated from `server/src/rpc/mod.rs` | Uses `ng_db::get_db()` |

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Token;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait, ModelTrait, Set};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, to_value};
use std::fmt;
use std::future::Future;
use std::sync::OnceLock;

// ── AuthChecker trait + global injection ──────────────────────────────

/// Trait for authentication checking.
///
/// Implementations verify raw token strings (`key:secret` or `username|password`)
/// and return the corresponding [`Token`] metadata.
pub trait AuthChecker: Send + Sync {
    /// Authenticate a raw token string and return the token metadata.
    fn check(&self, raw_token: &str) -> anyhow::Result<Token>;
}

static AUTH_CHECKER: OnceLock<Box<dyn AuthChecker>> = OnceLock::new();

/// Set the global auth checker.
///
/// Must be called once during server startup.
pub fn set_auth_checker(checker: Box<dyn AuthChecker>) {
    let _ = AUTH_CHECKER.set(checker);
}

/// Get the global auth checker.
///
/// Panics if not initialized -- call [`set_auth_checker`] first.
pub fn get_auth_checker() -> &'static dyn AuthChecker {
    AUTH_CHECKER
        .get()
        .expect("AuthChecker not initialized -- call set_auth_checker first")
        .as_ref()
}

// ── Helper: 一行从 Entity 全量加载 Models ────────────────────────────

/// 供 `DbBackedCache::load_all()` 一行调用。
pub async fn load_from_db<E>() -> anyhow::Result<Vec<E::Model>>
where
    E: EntityTrait + Send + Sync,
    E::Model: ModelTrait + Clone + Send + Sync + 'static,
{
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;
    E::find()
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load from DB: {e}"))
}

// ── DbBackedCache trait ───────────────────────────────────────────────

/// DB 全量加载缓存 trait.
///
/// 实现 trait 后配合 `make_global_cache!()` 消除重复的
/// `OnceLock + init/reload/global` 模板代码。
///
/// `reload_from_models` 使用 `&self`（内部可变性），
/// 因为 `OnceLock` 只提供共享引用。
#[allow(async_fn_in_trait)]
pub trait DbBackedCache: Sized + Send + Sync {
    /// 数据库 Model 类型
    type Model: ModelTrait + Clone + Send + Sync + 'static;

    /// 缓存名称（用于日志）
    fn cache_name() -> &'static str;

    /// 从 DB Model 列表构建**全新**的缓存实例。
    /// 用于首次 `init()` 和重复加载。
    fn build_cache(models: Vec<Self::Model>) -> Self;

    /// 用新的 Model 列表替换缓存的内部状态（使用内部可变性）。
    ///
    /// 每个缓存必须提供自己的实现，通过内部锁安全地替换数据。
    async fn reload_from_models(&self, models: Vec<Self::Model>);

    /// 从主 DB 加载全部记录。通常一行即可: `load_from_db::<MyEntity>()`
    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send;
}

// ── Macro: 生成 OnceLock 单例 + init/global/reload ────────────────────

/// 为 `DbBackedCache` 实现类型生成全局单例和 `init() / global() / reload()`.
///
/// ```ignore
/// make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);
/// ```
///
/// 生成:
/// - `static TOKEN_CACHE_GLOBAL: OnceLock<TokenCache>`
/// - `impl TokenCache { init, global, reload }`
#[macro_export]
macro_rules! make_global_cache {
    ($ty:ty, $global:ident) => {
        static $global: std::sync::OnceLock<$ty> = std::sync::OnceLock::new();

        impl $ty {
            /// 从 DB 全量加载并注册全局缓存。
            /// 如果已初始化则 reload（防止并发 init 冲突）。
            pub async fn init() -> anyhow::Result<()> {
                let __models =
                    <$ty as $crate::server::DbBackedCache>::load_all().await?;
                let __count = __models.len();
                let __instance =
                    <$ty as $crate::server::DbBackedCache>::build_cache(__models);
                if $global.set(__instance).is_err() {
                    tracing::warn!(
                        target: "cache",
                        name = <$ty as $crate::server::DbBackedCache>::cache_name(),
                        "already initialized, reloading"
                    );
                    return Self::reload().await;
                }
                tracing::info!(
                    target: "cache",
                    name = <$ty as $crate::server::DbBackedCache>::cache_name(),
                    count = __count,
                    "cache initialized"
                );
                Ok(())
            }

            /// 全局实例，panic 如果未 init.
            pub fn global() -> &'static Self {
                $global.get().expect(concat!(
                    stringify!($ty),
                    " not initialized — call ",
                    stringify!($ty),
                    "::init() first"
                ))
            }

            /// 从 DB 重新加载。未初始化时无操作。
            pub async fn reload() -> anyhow::Result<()> {
                let Some(__inst) = $global.get() else {
                    return Ok(());
                };
                let __models =
                    <$ty as $crate::server::DbBackedCache>::load_all().await?;
                let __count = __models.len();
                __inst.reload_from_models(__models).await;
                tracing::debug!(
                    target: "cache",
                    name = <$ty as $crate::server::DbBackedCache>::cache_name(),
                    count = __count,
                    "cache reloaded"
                );
                Ok(())
            }
        }
    };
}

// ── RPC tracing utilities ───────────────────────────────────────────

/// Lightweight extraction of `(token_key, username)` from a raw token string.
///
/// - Token mode (`key:secret`): returns `(key, "")`
/// - Auth mode (`username|password`): returns `("", username)`
/// - Fallback: returns `("???", "")`
///
/// Zero-allocation: returns borrowed slices into the original string.
pub fn token_identity(token: &str) -> (&str, &str) {
    token.find(':').map_or_else(
        || {
            token
                .find('|')
                .map_or(("???", ""), |pipe| ("", &token[..pipe]))
        },
        |colon| (&token[..colon], ""),
    )
}

/// A wrapper around `&RawValue` that truncates its `Display` output to 1024 bytes.
pub struct TruncatedRaw<'a>(pub &'a RawValue);

impl fmt::Display for TruncatedRaw<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MAX: usize = 1024;
        let s = self.0.get();
        if s.len() <= MAX {
            f.write_str(s)
        } else {
            let end = s.floor_char_boundary(MAX);
            f.write_str(&s[..end])?;
            write!(f, "[...{} bytes total]", s.len())
        }
    }
}

/// Common log pattern for RPC methods returning `RpcResult<Box<RawValue>>`.
///
/// Usage: `rpc_exec!(some_inner_call(args).await)`
///
/// Emits:
/// - `debug response=<truncated> "request completed"` on success
/// - `error error=<e> "request failed"` on failure
///
/// Note: the timing middleware already logs per-request timing at the
/// configured level, so the macro only logs the outcome.
///
/// Uses `target: "rpc"` intentionally -- this is cross-cutting RPC
/// infrastructure logging, distinct from domain-specific targets
/// (kv, token, `js_worker`, etc.).
#[macro_export]
macro_rules! rpc_exec {
    ($expr:expr) => {{
        match $expr {
            Ok(raw) => {
                tracing::debug!(
                    target: "rpc",
                    response = %$crate::server::TruncatedRaw(&raw),
                    "request completed"
                );
                Ok(raw)
            }
            Err(e) => {
                tracing::error!(target: "rpc", error = %e, "request failed");
                Err(e)
            }
        }
    }};
}

/// Common RPC helper trait providing DB access and serialization utilities.
///
/// Implement with an empty impl to bring methods into scope:
/// ```ignore
/// impl RpcHelper for MyRpcImpl {}
/// ```
pub trait RpcHelper {
    /// Serialize a value and wrap it in `sea_orm::Set` for active model fields.
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val).map(Set).map_err(|e| {
            NodegetError::SerializationError(format!("Serialization error: {e}")).into()
        })
    }

    /// Get the global database connection.
    ///
    /// Returns an error if the DB has not been initialized.
    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        ng_db::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}
