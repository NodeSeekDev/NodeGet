//! Server 端基础设施模块。
//!
//! 仅在 `server` feature 下可用，包含依赖 jsonrpsee、sea-orm、serde_json 的 trait 和宏。
//!
//! ## 内容
//!
//! | 条目 | 来源 | 说明 |
//! |------|------|------|
//! | `load_from_db` | 迁移自 `server/src/cache/mod.rs` | 使用 `ng_db::get_db()` |
//! | `DbBackedCache` trait | 迁移自 `server/src/cache/mod.rs` | 路径已调整 |
//! | `make_global_cache!` 宏 | 迁移自 `server/src/cache/mod.rs` | `$crate::server::DbBackedCache` |
//! | `token_identity` | 迁移自 `server/src/rpc/mod.rs` | 纯字符串逻辑 |
//! | `TruncatedRaw` | 迁移自 `server/src/rpc/mod.rs` | 使用 `serde_json::RawValue` |
//! | `rpc_exec!` 宏 | 迁移自 `server/src/rpc/mod.rs` | `$crate::server::TruncatedRaw` |
//! | `RpcHelper` trait | 迁移自 `server/src/rpc/mod.rs` | 使用 `ng_db::get_db()` |

use ng_core::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait, ModelTrait, Set};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, to_value};
use std::fmt;
use std::future::Future;

// ── 辅助函数：一行从 Entity 全量加载 Models ────────────────────────────

/// 从主数据库全量加载指定 Entity 的所有 Model 记录。
///
/// 供 `DbBackedCache::load_all()` 一行调用，内部步骤：
/// 1. 获取全局 DB 连接
/// 2. 执行 `E::find().all()` 查询
///
/// # Errors
///
/// 当数据库连接未初始化或查询失败时返回错误
pub async fn load_from_db<E>() -> anyhow::Result<Vec<E::Model>>
where
    E: EntityTrait + Send + Sync,
    E::Model: ModelTrait + Clone + Send + Sync + 'static,
{
    // 1. 获取全局 DB 连接
    let db = ng_db::get_db().ok_or_else(|| {
        NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
    })?;
    // 2. 执行全量查询
    E::find()
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load from DB: {e}"))
}

// ── DbBackedCache trait ───────────────────────────────────────────────

/// DB 全量加载缓存 trait。
///
/// 实现 trait 后配合 `make_global_cache!()` 宏可消除重复的
/// `OnceLock + init/reload/global` 模板代码。
///
/// `reload_from_models` 使用 `&self`（内部可变性），
/// 因为 `OnceLock` 只提供共享引用。
#[allow(async_fn_in_trait)]
pub trait DbBackedCache: Sized + Send + Sync {
    /// 数据库 Model 类型
    type Model: ModelTrait + Clone + Send + Sync + 'static;

    /// 缓存名称（用于日志标识）
    fn cache_name() -> &'static str;

    /// 从 DB Model 列表构建全新的缓存实例。
    ///
    /// 用于首次 `init()` 和重新加载时构建。
    fn build_cache(models: Vec<Self::Model>) -> Self;

    /// 用新的 Model 列表替换缓存的内部状态（使用内部可变性）。
    ///
    /// 每个缓存必须自行实现，通过内部锁（如 RwLock）安全地替换数据。
    async fn reload_from_models(&self, models: Vec<Self::Model>);

    /// 从主 DB 加载全部记录，通常一行即可：`load_from_db::<MyEntity>()`
    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send;
}

// ── 宏：生成 OnceLock 单例 + init/global/reload ────────────────────

/// 为 `DbBackedCache` 实现类型生成全局单例和 `init() / global() / reload()`。
///
/// ```ignore
/// make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);
/// ```
///
/// 生成内容：
/// - `static TOKEN_CACHE_GLOBAL: OnceLock<TokenCache>`
/// - `impl TokenCache { init, global, reload }`
#[macro_export]
macro_rules! make_global_cache {
    ($ty:ty, $global:ident) => {
        static $global: std::sync::OnceLock<$ty> = std::sync::OnceLock::new();

        impl $ty {
            /// 从 DB 全量加载并注册全局缓存。
            ///
            /// 若已初始化则改为 reload（防止并发 init 冲突），内部步骤：
            /// 1. 调用 `load_all()` 加载全部 Model
            /// 2. 调用 `build_cache()` 构建缓存实例
            /// 3. 尝试 `set()` 写入 OnceLock；若已被占用则改为 reload
            pub async fn init() -> anyhow::Result<()> {
                let __models =
                    <$ty as $crate::server::DbBackedCache>::load_all().await?;
                let __count = __models.len();
                let __instance =
                    <$ty as $crate::server::DbBackedCache>::build_cache(__models);
                // 并发 init 时 OnceLock 已被占用，回退到 reload
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

            /// 获取全局实例。
            ///
            /// 若未调用 `init()` 则返回 `None`。
            pub fn global() -> Option<&'static Self> {
                $global.get()
            }

            /// 从 DB 重新加载缓存数据。
            ///
            /// 若全局实例尚未初始化则无操作，内部步骤：
            /// 1. 获取全局实例引用
            /// 2. 调用 `load_all()` 重新加载全部 Model
            /// 3. 调用 `reload_from_models()` 替换内部状态
            pub async fn reload() -> anyhow::Result<()> {
                // 未初始化时跳过，避免空 reload
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

// ── RPC 日志工具 ───────────────────────────────────────────────────

/// 从原始 Token 字符串中提取 `(token_key, username)`。
///
/// - Token 模式（`key:secret`）：返回 `(key, "")`
/// - Auth 模式（`username|password`）：返回 `("", username)`
/// - 无法识别时：返回 `("???", "")`
///
/// 零分配：返回借用的字符串切片，指向原始字符串内部。
pub fn token_identity(token: &str) -> (&str, &str) {
    token.find(':').map_or_else(
        || {
            // 未找到冒号，尝试管道符分隔（Auth 模式）
            token
                .find('|')
                .map_or(("???", ""), |pipe| ("", &token[..pipe]))
        },
        |colon| (&token[..colon], ""),
    )
}

/// `&RawValue` 的截断 Display 包装，输出超过 1024 字节时截断并附加长度提示。
pub struct TruncatedRaw<'a>(pub &'a RawValue);

impl fmt::Display for TruncatedRaw<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MAX: usize = 1024;
        let s = self.0.get();
        if s.len() <= MAX {
            // 未超限，原样输出
            f.write_str(s)
        } else {
            // 超限时截断到字符边界，附加总字节数提示
            let end = s.floor_char_boundary(MAX);
            f.write_str(&s[..end])?;
            write!(f, "[...{} bytes total]", s.len())
        }
    }
}

/// RPC 方法返回 `RpcResult<Box<RawValue>>` 的统一日志宏。
///
/// 用法：`rpc_exec!(some_inner_call(args).await)`
///
/// 输出行为：
/// - 成功：`debug response=<truncated> "request completed"`
/// - 失败：`error error=<e> "request failed"`
///
/// 注意：计时中间件已按配置级别记录请求耗时，本宏仅记录结果。
///
/// 使用 `target: "rpc"` 作为跨领域 RPC 基础设施日志，
/// 区别于领域特定 target（kv、token、js_worker 等）。
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

/// RPC 公共辅助 trait，提供 DB 访问与序列化工具方法。
///
/// 使用空 impl 引入方法：
/// ```ignore
/// impl RpcHelper for MyRpcImpl {}
/// ```
pub trait RpcHelper {
    /// 将值序列化为 JSON 并包装为 `sea_orm::Set`，用于 ActiveModel 字段赋值。
    ///
    /// - `val` — 待序列化的值
    /// - 返回 `ActiveValue<Value>`，失败时返回序列化错误
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val).map(Set).map_err(|e| {
            NodegetError::SerializationError(format!("Serialization error: {e}")).into()
        })
    }

    /// 获取全局数据库连接。
    ///
    /// # Errors
    ///
    /// 当 DB 未初始化时返回错误。
    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        ng_db::get_db()
            .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}
