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
use std::sync::{LazyLock, OnceLock, RwLock};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

mod config_access;
pub mod dry_run;
mod monitoring;
mod ntp;
mod rpc;
mod tasks;

static AGENT_ARGS: OnceLock<AgentArgs> = OnceLock::new();
static AGENT_CONFIG: OnceLock<RwLock<AgentConfig>> = OnceLock::new();
pub(crate) static RELOAD_NOTIFY: LazyLock<Notify> = LazyLock::new(Notify::new);
static NTP_INIT_DONE: OnceLock<bool> = OnceLock::new();

fn parse_log_level(config: &AgentConfig) -> anyhow::Result<Level> {
    let log_level = config
        .log_level
        .as_ref()
        .ok_or_else(|| NodegetError::ParseError("log_level is not set".to_owned()))?;

    Level::from_str(log_level)
        .map_err(|e| NodegetError::ParseError(format!("Invalid log_level: {e}")))
        .map_err(Into::into)
}

fn update_global_config(config: AgentConfig) -> anyhow::Result<()> {
    if let Some(lock) = AGENT_CONFIG.get() {
        let mut guard = lock.write().map_err(|e| {
            NodegetError::Other(format!("Failed to lock AGENT_CONFIG for write: {e}"))
        })?;
        *guard = config;
        return Ok(());
    }

    AGENT_CONFIG
        .set(RwLock::new(config))
        .map_err(|_| NodegetError::Other("Failed to set AGENT_CONFIG".to_owned()).into())
}

fn abort_handles(handles: &mut Vec<JoinHandle<()>>) {
    for handle in handles.drain(..) {
        handle.abort();
    }
}

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

    AGENT_ARGS.set(args.clone()).unwrap();

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

        update_global_config(config.clone())?;

        let servers = config.server.clone().ok_or_else(|| {
            NodegetError::ConfigNotFound("No server configuration found".to_owned())
        })?;

        dry_run().await;

        if args.dry_run {
            exit(0);
        }

        let connect_timeout = config.connect_timeout_duration();
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
