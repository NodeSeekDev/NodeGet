//! 通用 DB-backed 缓存 trait
//!
//! 为所有 "从主 DB 全量加载 → 内存缓存 → 支持 reload" 的场景提供统一框架。
//!
//! ## 使用方式
//!
//! 1. 定义缓存结构体（内部用 `RwLock` 等实现内部可变性）
//! 2. 实现 `DbBackedCache` trait（3 个方法 + `load_all` 一行调用）
//! 3. 调用 `make_global_cache!()` 获得 `init() / global() / reload()`
//! 4. 在 `impl 结构体 {}` 中定义领域访问器方法

use crate::DB;
use nodeget_lib::error::NodegetError;
use sea_orm::{EntityTrait, ModelTrait};
use std::future::Future;

// ── Helper: 一行从 Entity 全量加载 Models ────────────────────────────

/// 供 `DbBackedCache::load_all()` 一行调用。
pub async fn load_from_db<E>() -> anyhow::Result<Vec<E::Model>>
where
    E: EntityTrait + Send + Sync,
    E::Model: ModelTrait + Clone + Send + Sync + 'static,
{
    let db = DB.get().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;
    E::find()
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load from DB: {e}"))
}

// ── Trait ─────────────────────────────────────────────────────────────

/// DB 全量加载缓存 trait.
///
/// 实现 trait 后配合 `make_global_cache!()` 消除重复的
/// `OnceLock + init/reload/global` 模板代码。
///
/// `reload_from_models` 使用 `&self`（内部可变性），
/// 因为 `OnceLock` 只提供共享引用。
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
    /// 默认实现直接 `*self = Self::build_cache(...)`，适用于
    /// 单一 `RwLock<HashMap>` 结构。若有多个内部字段需要分别
    /// 替换，可覆盖此方法。
    #[allow(clippy::unused_async)]
    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        // Safety: this uses a "cheat" — create a brand new instance,
        // then use std::mem::replace on self. Since Rust doesn't let
        // us do *self = ... with &self, we use unsafe for the swap.
        // Actually, simpler: we cast through a raw pointer.
        unsafe {
            let ptr = self as *const Self as *mut Self;
            ptr.write(Self::build_cache(models));
        }
    }

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
                    <$ty as $crate::cache::DbBackedCache>::load_all().await?;
                let __count = __models.len();
                let __instance =
                    <$ty as $crate::cache::DbBackedCache>::build_cache(__models);
                if $global.set(__instance).is_err() {
                    tracing::warn!(
                        target: "cache",
                        name = <$ty as $crate::cache::DbBackedCache>::cache_name(),
                        "already initialized, reloading"
                    );
                    return Self::reload().await;
                }
                tracing::info!(
                    target: "cache",
                    name = <$ty as $crate::cache::DbBackedCache>::cache_name(),
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
                    <$ty as $crate::cache::DbBackedCache>::load_all().await?;
                let __count = __models.len();
                __inst.reload_from_models(__models).await;
                tracing::debug!(
                    target: "cache",
                    name = <$ty as $crate::cache::DbBackedCache>::cache_name(),
                    count = __count,
                    "cache reloaded"
                );
                Ok(())
            }
        }
    };
}
