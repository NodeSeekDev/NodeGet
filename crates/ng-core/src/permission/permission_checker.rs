//! 统一权限校验 trait 与全局注入。
//!
//! 提供统一的 [`PermissionChecker`] trait，替代原先散布在 ng-db、ng-kv、ng-static、
//! ng-js-worker、ng-terminal、ng-task 六个 crate 中的重复 trait 定义。
//!
//! 所有方法最终委托给 `ng_token` 的同名函数，实现类仅需一份。
//! 服务器二进制在启动时调用 [`set_permission_checker`] 注入具体实现，
//! 各业务 crate 通过 [`get_permission_checker`] 获取全局实例，
//! 或使用 [`require_permission_checker`] 获取并附带统一错误信息。

use super::data_structure::{Permission, Scope, Token};
use super::token_auth::TokenOrAuth;
use crate::error::NodegetError;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

// ── PermissionChecker trait ─────────────────────────────────────────

/// 统一权限校验 trait。
///
/// 合并了原先 `ng_db::rpc::AuthProvider`、`ng_kv::TokenPermissionChecker`、
/// `ng_static::auth::TokenPermissionChecker`、`ng_js_worker::TokenPermissionChecker`、
/// `ng_terminal::TokenPermissionChecker`、`ng_task::TaskAuthProvider` 六个 trait。
///
/// 实现类验证 Token/Auth 是否满足给定的 Scope + Permission 约束，
/// 检查是否为 SuperToken，以及获取 Token 元数据。
pub trait PermissionChecker: Send + Sync + 'static {
    /// 检查 Token/Auth 是否满足给定的 Scope 和 Permission 约束。
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>;

    /// 检查 Token/Auth 是否为 SuperToken。
    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>;

    /// 获取 Token/Auth 的元数据信息。
    fn get_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Token>> + Send>>;
}

// ── 全局注入 ──────────────────────────────────────────────────────

/// 全局 PermissionChecker 单例，服务器启动时通过 `set_permission_checker` 注入
static PERMISSION_CHECKER: OnceLock<std::sync::Arc<dyn PermissionChecker>> = OnceLock::new();

/// 注入全局 PermissionChecker 实现，仅应在服务器启动时调用一次。
///
/// 重复调用时输出警告日志并忽略第二次注册。
pub fn set_permission_checker(checker: std::sync::Arc<dyn PermissionChecker>) {
    if PERMISSION_CHECKER.set(checker).is_err() {
        tracing::warn!(target: "permission", "PermissionChecker already initialized, ignoring duplicate registration");
    }
}

/// 获取已注入的全局 PermissionChecker，未初始化时返回 None
pub fn get_permission_checker() -> Option<&'static std::sync::Arc<dyn PermissionChecker>> {
    PERMISSION_CHECKER.get()
}

/// 获取全局 PermissionChecker，未初始化时返回统一错误。
///
/// 供各业务 crate 的 auth 模块调用，避免重复编写 `ok_or_else` 错误构造。
pub fn require_permission_checker() -> anyhow::Result<&'static std::sync::Arc<dyn PermissionChecker>> {
    get_permission_checker().ok_or_else(|| {
        NodegetError::ConfigNotFound("PermissionChecker not initialized".to_owned()).into()
    })
}
