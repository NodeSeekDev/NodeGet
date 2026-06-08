//! RPC 层公共基础设施
//!
//! 提供 RPC 日志宏（`rpc_exec!`）、请求追踪工具（`token_identity`、`TruncatedRaw`）、
//! `RpcHelper` trait 和错误转换函数。
//!
//! 权限校验已统一至 `ng_core::permission::permission_checker::PermissionChecker`，
//! 各 RPC 命名空间通过 `ng_core::permission::permission_checker::get_permission_checker()`
//! 获取认证能力。

use crate::get_db;
use ng_core::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, to_value};
use std::fmt;

#[cfg(feature = "server")]
pub mod db;
#[cfg(feature = "server")]
pub mod nodeget;

// ── RPC 追踪与日志工具 ────────────────────────────────────────────

/// 从原始 token 字符串提取身份标识，用于日志追踪
///
/// - Token 模式（`key:secret`）：返回 `(key, "")`
/// - Auth 模式（`username|password`）：返回 `("", username)`
/// - 无法识别时：返回 `("???", "")`
///
/// 零分配：返回的切片借用了原始字符串
#[must_use]
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

/// `RawValue` 的截断显示包装器，`Display` 输出超过 1024 字节时截断并附加长度提示
///
/// 用于 RPC 响应日志，避免超大 JSON 占满日志输出
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

/// RPC 方法返回值统一日志宏
///
/// 用法：`rpc_exec!(some_inner_call(args).await)`
///
/// 行为：
/// - 成功时输出 `debug response=<截断> "request completed"`
/// - 失败时输出 `error error=<e> "request failed"`
#[macro_export]
macro_rules! rpc_exec {
    ($expr:expr) => {{
        match $expr {
            Ok(raw) => {
                tracing::debug!(target: "rpc", response = %$crate::rpc::TruncatedRaw(&raw), "request completed");
                Ok(raw)
            }
            Err(e) => {
                tracing::error!(target: "rpc", error = %e, "request failed");
                Err(e)
            }
        }
    }};
}

/// RPC 公共辅助 trait，提供序列化和数据库连接获取的快捷方法
pub trait RpcHelper {
    /// 将可序列化值转换为 `SeaORM` `ActiveValue<Value>`，用于模型字段设置
    ///
    /// # Errors
    ///
    /// 当序列化失败时返回 `SerializationError`
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val).map(Set).map_err(|e| {
            NodegetError::SerializationError(format!("Serialization error: {e}")).into()
        })
    }

    /// 获取全局主库连接，未初始化时返回 `DatabaseError`
    ///
    /// # Errors
    ///
    /// 当主库连接未初始化时返回 `DatabaseError`
    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}

/// 将 anyhow 错误转换为 JSON-RPC `ErrorObject`
///
/// - `e` — 任意 anyhow 错误
/// - 返回值：包含 `NodeGet` 错误码和消息的 JSON-RPC 错误对象
#[must_use]
pub fn to_rpc_error(e: &anyhow::Error) -> jsonrpsee::types::ErrorObject<'static> {
    let nodeget_err = ng_core::error::anyhow_to_nodeget_error(e);
    jsonrpsee::types::ErrorObject::owned(
        nodeget_err.error_code() as i32,
        format!("{nodeget_err}"),
        None::<()>,
    )
}
