#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::doc_markdown
)]

// 配置管理模块，处理 Agent 和 Server 的配置
pub mod config;
// 命令行参数解析模块
pub mod args_parse;

// Server 端 RPC 配置操作（read_config / edit_config）
#[cfg(feature = "server")]
pub mod server_rpc;

// ── 全局单例 ──────────────────────────────────────────────────────
// SERVER_CONFIG: 全局服务器配置（OnceLock + RwLock）
// RELOAD_NOTIFY: 配置热重载通知（OnceLock + Notify）
// SERVER_CONFIG_PATH: 配置文件路径

use config::server::ServerConfig;
use std::sync::{OnceLock, RwLock};

static SERVER_CONFIG: OnceLock<RwLock<ServerConfig>> = OnceLock::new();
static SERVER_CONFIG_PATH: OnceLock<String> = OnceLock::new();
static RELOAD_NOTIFY: OnceLock<tokio::sync::Notify> = OnceLock::new();

/// 获取全局 SERVER_CONFIG 的只读引用
///
/// 返回 `None` 表示尚未初始化
#[must_use]
pub fn get_server_config() -> Option<&'static RwLock<ServerConfig>> {
    SERVER_CONFIG.get()
}

/// 获取全局 SERVER_CONFIG_PATH
#[must_use]
pub fn get_server_config_path() -> Option<&'static str> {
    SERVER_CONFIG_PATH.get().map(String::as_str)
}

/// 获取全局 RELOAD_NOTIFY
#[must_use]
pub fn get_reload_notify() -> Option<&'static tokio::sync::Notify> {
    RELOAD_NOTIFY.get()
}

/// 设置全局 SERVER_CONFIG
///
/// - 若已初始化：写入新值并返回 `Ok(())`
/// - 若未初始化：首次设置并返回 `Ok(())`
/// - 若并发首次设置失败：返回 `Err`
///
/// # Errors
///
/// 当并发首次设置导致 `OnceLock` 竞争失败时返回错误
pub fn set_server_config(config: ServerConfig) -> anyhow::Result<()> {
    if let Some(lock) = SERVER_CONFIG.get() {
        {
            let mut guard = lock.write().map_err(|e| anyhow::anyhow!("{e}"))?;
            *guard = config;
        }
        return Ok(());
    }

    SERVER_CONFIG
        .set(RwLock::new(config))
        .map_err(|_| anyhow::anyhow!("Failed to set SERVER_CONFIG"))?;
    Ok(())
}

/// 设置全局 SERVER_CONFIG_PATH
///
/// # Errors
///
/// 当 `OnceLock` 已被设置时返回错误
pub fn set_server_config_path(path: String) -> Result<(), String> {
    SERVER_CONFIG_PATH
        .set(path)
        .map_err(|_| "SERVER_CONFIG_PATH already set".to_owned())
}

/// 初始化 RELOAD_NOTIFY（若尚未初始化）
pub fn init_reload_notify() {
    RELOAD_NOTIFY.get_or_init(tokio::sync::Notify::new);
}
