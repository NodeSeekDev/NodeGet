//! `crontab` RPC 命名空间实现：定时任务的 CRUD 与启用/禁用操作。
//!
//! 使用 jsonrpsee `#[rpc]` 宏定义 trait，`CrontabRpcImpl` 实现各方法，
//! 每个方法通过 `rpc_exec!` 宏统一日志与错误处理，
//! 具体业务逻辑委托到子模块（create、edit、delete、get、set_enable）。

use crate::CronType;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod create;
mod delete;
mod edit;
mod get;
mod set_enable;

/// 校验 crontab 名称合法性。
///
/// 采用黑名单模式：仅禁止路径分隔符和控制字符，其余（含空格、emoji、中文等）均允许。
fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        return Err(ng_core::error::NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
    }
    if name.chars().count() > 128 {
        return Err(ng_core::error::NodegetError::InvalidInput("name too long (max 128 chars)".to_owned()).into());
    }
    let invalid = name.chars().any(|c| {
        // 禁止：路径分隔符、控制字符（含 null）
        c == '/' || c == '\\' || c.is_control()
    });
    if invalid {
        return Err(ng_core::error::NodegetError::InvalidInput(
            "name contains invalid characters (path separators and control characters not allowed)".to_owned(),
        )
        .into());
    }
    Ok(())
}

/// `crontab` RPC trait 定义，使用 jsonrpsee `#[rpc]` 宏自动生成 Server 端调度代码。
/// 命名空间为 `crontab`，分隔符为 `_`（自定义 jsonrpsee fork）。
#[rpc(server, namespace = "crontab")]
pub trait Rpc {
    /// 创建定时任务
    #[method(name = "create")]
    async fn create(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>>;

    /// 编辑定时任务
    #[method(name = "edit")]
    async fn edit(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>>;

    /// 获取定时任务列表
    #[method(name = "get")]
    async fn get(&self, token: String) -> RpcResult<Box<RawValue>>;

    /// 删除定时任务
    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 设置定时任务启用/禁用状态
    #[method(name = "set_enable")]
    async fn set_enable(
        &self,
        token: String,
        name: String,
        enable: bool,
    ) -> RpcResult<Box<RawValue>>;
}

/// `crontab` RPC 实现结构体，空载体（所有状态通过全局单例获取）。
pub struct CrontabRpcImpl;

impl RpcHelper for CrontabRpcImpl {}

#[async_trait]
impl RpcServer for CrontabRpcImpl {
    async fn create(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::create", token_key = tk, username = un, name = %name, cron_expression = %cron_expression, cron_type = ?cron_type);
        async { rpc_exec!(create::create(token, name, cron_expression, cron_type).await) }
            .instrument(span)
            .await
    }

    async fn edit(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::edit", token_key = tk, username = un, name = %name, cron_expression = %cron_expression, cron_type = ?cron_type);
        async { rpc_exec!(edit::edit(token, name, cron_expression, cron_type).await) }
            .instrument(span)
            .await
    }

    async fn get(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span =
            tracing::info_span!(target: "crontab", "crontab::get", token_key = tk, username = un);
        async { rpc_exec!(get::get(token).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    async fn set_enable(
        &self,
        token: String,
        name: String,
        enable: bool,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::set_enable", token_key = tk, username = un, name = %name, enable = enable);
        async { rpc_exec!(set_enable::set_enable(token, name, enable).await) }
            .instrument(span)
            .await
    }
}

/// 构建并返回 `crontab` RPC 模块。
pub fn rpc_module() -> jsonrpsee::RpcModule<CrontabRpcImpl> {
    CrontabRpcImpl.into_rpc()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ng_core::error::NodegetError;

    // ── validate_name: happy path ────────────────────────────────

    #[test]
    fn validate_name_valid_simple() {
        assert!(validate_name("mycron").is_ok());
    }

    #[test]
    fn validate_name_valid_with_underscore() {
        assert!(validate_name("my_cron").is_ok());
    }

    #[test]
    fn validate_name_valid_with_dash() {
        assert!(validate_name("my-cron").is_ok());
    }

    #[test]
    fn validate_name_valid_mixed() {
        assert!(validate_name("cron_1-2").is_ok());
    }

    #[test]
    fn validate_name_valid_all_alphanumeric() {
        assert!(validate_name("abc123").is_ok());
    }

    #[test]
    fn validate_name_valid_single_char() {
        assert!(validate_name("a").is_ok());
    }

    #[test]
    fn validate_name_valid_single_digit() {
        assert!(validate_name("1").is_ok());
    }

    #[test]
    fn validate_name_valid_uppercase() {
        assert!(validate_name("CronABC").is_ok());
    }

    #[test]
    fn validate_name_accepts_exactly_128_chars() {
        let name = "a".repeat(128);
        assert!(validate_name(&name).is_ok());
    }

    #[test]
    fn validate_name_accepts_chinese() {
        assert!(validate_name("电信ping").is_ok());
    }

    #[test]
    fn validate_name_accepts_mixed_chinese_ascii() {
        assert!(validate_name("定时任务_1").is_ok());
    }

    #[test]
    fn validate_name_accepts_space() {
        assert!(validate_name("my cron").is_ok());
    }

    #[test]
    fn validate_name_accepts_emoji() {
        assert!(validate_name("🇭🇰 HKG ping").is_ok());
    }

    #[test]
    fn validate_name_accepts_flag_emoji_full() {
        // Issue #160: 国旗 emoji + 空格 + 短横线
        assert!(validate_name("TCPing - 🇭🇰 HKG - Peekabo CDN Edge IPv4").is_ok());
    }

    #[test]
    fn validate_name_accepts_dot() {
        assert!(validate_name("cron.name").is_ok());
    }

    #[test]
    fn validate_name_accepts_special_chars() {
        // emoji、标点、空格均允许
        assert!(validate_name("任务❌").is_ok());
        assert!(validate_name("ping!").is_ok());
        assert!(validate_name("a@b#c").is_ok());
    }

    // ── validate_name: empty ──────────────────────────────────────

    #[test]
    fn validate_name_rejects_empty() {
        let result = validate_name("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("empty")));
    }

    // ── validate_name: too long ────────────────────────────────────

    #[test]
    fn validate_name_rejects_too_long() {
        let long_name = "a".repeat(129);
        let result = validate_name(&long_name);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let nodeget_err = err.downcast_ref::<NodegetError>().unwrap();
        assert!(matches!(nodeget_err, NodegetError::InvalidInput(msg) if msg.contains("128")));
    }

    // ── validate_name: invalid characters (path separators & control) ──

    #[test]
    fn validate_name_rejects_slash() {
        assert!(validate_name("path/cron").is_err());
    }

    #[test]
    fn validate_name_rejects_backslash() {
        assert!(validate_name("path\\cron").is_err());
    }

    #[test]
    fn validate_name_rejects_control_char() {
        assert!(validate_name("cron\tname").is_err());
        assert!(validate_name("cron\nname").is_err());
    }

    #[test]
    fn validate_name_rejects_null() {
        assert!(validate_name("cron\0name").is_err());
    }
}
