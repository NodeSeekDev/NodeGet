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

use crate::rpc::handle_error_message;
use crate::rpc::monitoring_data_report::{
    handle_dynamic_monitoring_data_report, handle_static_monitoring_data_report,
};
use crate::tasks::handle_task;
use log::{Level, error, info};
use nodeget_lib::args_parse::agent::AgentArgs;
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::utils::set_ntp_offset_ms;
use nodeget_lib::utils::uuid::compare_uuid;
use nodeget_lib::utils::version::NodeGetVersion;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

mod monitoring;
mod ntp;
mod rpc;
mod tasks;

static AGENT_ARGS: OnceLock<AgentArgs> = OnceLock::new();
static AGENT_CONFIG: OnceLock<RwLock<AgentConfig>> = OnceLock::new();
pub(crate) static RELOAD_NOTIFY: OnceLock<Notify> = OnceLock::new();
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
    println!("Starting nodeget-agent");

    let args = AgentArgs::par();

    {
        if args.version {
            let version = NodeGetVersion::get();
            println!("{version}");
            return Ok(());
        }
    }

    AGENT_ARGS.set(args.clone()).unwrap();

    RELOAD_NOTIFY.get_or_init(Notify::new);
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

        if let Err(e) = compare_uuid(config.agent_uuid) {
            error!("UUID comparison failed: {e}");
        }

        info!("Starting nodeget-agent with config: {config:?}");

        // 仅在首次启动时查询 NTP 时间偏移，避免热重载时覆盖已有偏移导致时间跳变
        if NTP_INIT_DONE.get().is_none() {
            let ntp_server = config
                .ntp_server
                .as_deref()
                .unwrap_or("pool.ntp.org");
            let ntp_offset = ntp::fetch_ntp_offset(ntp_server).await;
            println!("NTP time offset: {ntp_offset} ms");
            set_ntp_offset_ms(ntp_offset);
            let _ = NTP_INIT_DONE.set(true);
        }

        update_global_config(config.clone())?;

        let servers = config.server.clone().ok_or_else(|| {
            NodegetError::ConfigNotFound("No server configuration found".to_owned())
        })?;

        let mut handles = rpc::multi_server::init_connections(servers).await;

        handles.push(tokio::spawn(async {
            handle_static_monitoring_data_report().await;
        }));

        handles.push(tokio::spawn(async {
            handle_dynamic_monitoring_data_report().await;
        }));

        handles.push(tokio::spawn(async {
            handle_error_message().await;
        }));

        handles.push(tokio::spawn(async {
            handle_task().await;
        }));

        tokio::select! {
            ctrl_c_result = tokio::signal::ctrl_c() => {
                ctrl_c_result
                    .map_err(|e| NodegetError::Other(format!("Failed to listen for ctrl_c: {e}")))?;
                abort_handles(&mut handles);
                break;
            }
            () = RELOAD_NOTIFY.get().expect("Reload notify not initialized").notified() => {
                info!("Config reload requested, restarting runtime tasks...");
                abort_handles(&mut handles);
            }
        }
    }

    Ok(())
}
