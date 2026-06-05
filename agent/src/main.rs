//! `NodeGet` Agent 入口模块。
//!
//! 负责启动 agent 进程、解析命令行参数与配置文件、初始化日志和 NTP 时间校准，
//! 并驱动与多服务器的 WebSocket 连接及监控数据上报循环。
//! 配置热重载通过 [`RELOAD_NOTIFY`] 信号触发，主循环 abort 所有运行时任务后
//! 重新读取配置并重建连接。

#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::significant_drop_tightening,
    dead_code
)]

use crate::dry_run::dry_run;
use crate::rpc::handle_error_message;
use crate::rpc::monitoring_data_report::{
    handle_dynamic_monitoring_data_report, handle_static_monitoring_data_report,
};
use crate::tasks::handle_task;
use log::{Level, info};
use ng_config::args_parse::agent::AgentArgs;
use ng_config::config::agent::AgentConfig;
use ng_core::error::NodegetError;
use ng_core::utils::set_ntp_offset_ms;
use ng_core::utils::version::NodeGetVersion;
use std::process::exit;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, OnceLock, RwLock};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

mod config_access;
pub mod dry_run;
mod monitoring;
mod ntp;
mod rpc;
mod tasks;

/// 命令行参数全局单例，启动时设置一次，之后只读。[`AGENT_ARGS`]
static AGENT_ARGS: OnceLock<AgentArgs> = OnceLock::new();
/// 全局配置 `RwLock` 单例，支持热重载时写入新配置。`Arc<AgentConfig>` 使 `get_agent_config()` 返回 O(1) 的 Arc clone。
static AGENT_CONFIG: OnceLock<RwLock<Arc<AgentConfig>>> = OnceLock::new();
/// 配置热重载通知信号；`EditConfig` 任务写入配置文件后 notify，主循环收到后 abort 并重建。
pub(crate) static RELOAD_NOTIFY: LazyLock<Notify> = LazyLock::new(Notify::new);
/// NTP 初始化完成标记，防止热重载时覆盖已有偏移导致时间跳变。
static NTP_INIT_DONE: OnceLock<bool> = OnceLock::new();

/// 从配置中解析日志级别。
///
/// - `config` - Agent 配置引用
///
/// 返回解析后的 [`Level`]；配置缺失或格式非法时返回错误。
fn parse_log_level(config: &AgentConfig) -> anyhow::Result<Level> {
    let log_level = config
        .log_level
        .as_ref()
        .ok_or_else(|| NodegetError::ParseError("log_level is not set".to_owned()))?;

    Level::from_str(log_level)
        .map_err(|e| NodegetError::ParseError(format!("Invalid log_level: {e}")))
        .map_err(Into::into)
}

/// 更新全局配置单例。
///
/// - `config` - 新的 Agent 配置
///
/// 若 `AGENT_CONFIG` 已初始化则写入替换，否则首次设置。RwLock 毒化时返回错误。
fn update_global_config(config: AgentConfig) -> anyhow::Result<()> {
    if let Some(lock) = AGENT_CONFIG.get() {
        let mut guard = lock.write().map_err(|e| {
            NodegetError::Other(format!("Failed to lock AGENT_CONFIG for write: {e}"))
        })?;
        *guard = Arc::new(config);
        return Ok(());
    }

    AGENT_CONFIG
        .set(RwLock::new(Arc::new(config)))
        .map_err(|_| NodegetError::Other("Failed to set AGENT_CONFIG".to_owned()).into())
}

/// Abort 所有给定的 `JoinHandle`（用于热重载前清理运行时任务）。
///
/// - `handles` - 可变的 `JoinHandle` 向量，调用后会被清空
fn abort_handles(handles: &mut Vec<JoinHandle<()>>) {
    for handle in handles.drain(..) {
        handle.abort();
    }
}

/// Agent 进程入口。
///
/// 1. 安装 rustls crypto provider（容忍重复安装）
/// 2. 解析命令行参数，处理 `--version` 后退出
/// 3. 加载配置文件并初始化日志
/// 4. 首次启动时查询 NTP 偏移
/// 5. 建立与各服务器的 WebSocket 连接
/// 6. 启动静态/动态监控数据上报及错误消息/任务处理循环
/// 7. 监听 Ctrl+C 或热重载信号，前者退出、后者 abort 任务后重新进入循环
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // rustls crypto provider 只能安装一次；`tasks/ip.rs` 懒加载路径也会尝试安装并用
    // `let _ =` 吞错，这里同样忽略重复安装失败以保持两处策略一致，避免日后某个第三方
    // 依赖也抢先安装后整个 agent 直接 panic。
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 此处不再 println! 启动横幅：config/logger 初始化完成后会 `info!("Starting nodeget-agent with config: {config:?}")`
    // 提供等价信号；启动早期失败也会由 `main` 的 `anyhow::Result` 把错误输出到 stderr。

    let args = AgentArgs::par();

    {
        if args.version {
            let version = NodeGetVersion::get();
            println!("{version}");
            return Ok(());
        }
    }

    AGENT_ARGS.set(args).unwrap();

    let mut logger_initialized = false;

    loop {
        let config = AgentConfig::get_and_parse_config(AGENT_ARGS.get().unwrap().config.clone())
            .await
            .map_err(|e| NodegetError::ConfigNotFound(format!("Failed to load config: {e}")))?;

        let level = parse_log_level(&config)?;

        if logger_initialized {
            log::set_max_level(level.to_level_filter());
        } else {
            simple_logger::init_with_level(level)
                .map_err(|e| NodegetError::Other(format!("Failed to init logger: {e}")))?;
            logger_initialized = true;
        }

        info!("Starting nodeget-agent with config: {config:?}");

        // 仅在首次启动时查询 NTP 时间偏移，避免热重载时覆盖已有偏移导致时间跳变
        if NTP_INIT_DONE.get().is_none() {
            let ntp_server = config.ntp_server_or_default();
            let ntp_offset = ntp::fetch_ntp_offset(ntp_server).await;
            info!("NTP time offset: {ntp_offset} ms");
            set_ntp_offset_ms(ntp_offset);
            let _ = NTP_INIT_DONE.set(true);
        }

        let servers = config.server.clone().ok_or_else(|| {
            NodegetError::ConfigNotFound("No server configuration found".to_owned())
        })?;

        let connect_timeout = config.connect_timeout_duration();

        update_global_config(config)?;

        dry_run().await;

        if AGENT_ARGS.get().unwrap().dry_run {
            exit(0);
        }

        let mut handles = rpc::multi_server::init_connections(servers, connect_timeout).await;

        handles.push(tokio::spawn(handle_static_monitoring_data_report()));
        handles.push(tokio::spawn(handle_dynamic_monitoring_data_report()));
        handles.push(tokio::spawn(handle_error_message()));
        handles.push(tokio::spawn(handle_task()));

        tokio::select! {
            ctrl_c_result = tokio::signal::ctrl_c() => {
                ctrl_c_result
                    .map_err(|e| NodegetError::Other(format!("Failed to listen for ctrl_c: {e}")))?;
                abort_handles(&mut handles);
                break;
            }
            () = RELOAD_NOTIFY.notified() => {
                info!("Config reload requested, restarting runtime tasks...");
                abort_handles(&mut handles);
            }
        }
    }

    Ok(())
}
