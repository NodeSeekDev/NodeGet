#![feature(duration_millis_float)]
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
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::utils::uuid::compare_uuid;
use std::str::FromStr;
use std::sync::OnceLock;

// 监控模块
mod monitoring;
// RPC 模块
mod rpc;
// 任务模块
mod tasks;

// 全局代理配置静态变量
static AGENT_CONFIG: OnceLock<AgentConfig> = OnceLock::new();

// 主函数，程序入口点
//
// 该函数负责初始化代理配置、设置日志级别、启动监控数据上报、任务处理等功能
//
// # 详细流程
// 1. 加载配置文件
// 2. 初始化日志系统
// 3. 设置全局配置
// 4. 初始化与服务器的连接
// 5. 启动各种异步任务（监控数据上报、任务处理等）
// 6. 等待 Ctrl+C 信号退出
//
// # Errors
//
// 当配置加载失败、日志初始化失败或关键组件初始化失败时返回错误
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting nodeget-agent");

    // 从配置文件加载代理配置
    let config = AgentConfig::get_and_parse_config("./config.toml")
        .await
        .map_err(|e| NodegetError::ConfigNotFound(format!("Failed to load config: {e}")))?;

    // 使用配置的日志级别初始化简单日志系统
    let log_level = config.log_level.as_ref()
        .ok_or(NodegetError::ParseError("log_level is not set".to_owned()))?;
    simple_logger::init_with_level(Level::from_str(log_level)
        .map_err(|e| NodegetError::ParseError(format!("Invalid log_level: {e}")))?)
        .map_err(|e| NodegetError::Other(format!("Failed to init logger: {e}")))?;

    // 比较并验证代理 UUID
    if let Err(e) = compare_uuid(config.agent_uuid) {
        error!("UUID comparison failed: {e}");
    }

    info!("Starting nodeget-agent with config: {config:?}");

    // 将配置设置到全局静态变量中
    AGENT_CONFIG.set(config).map_err(|_| NodegetError::Other("Failed to set AGENT_CONFIG".to_owned()))?;

    //////////

    // 初始化与多个服务器的连接
    let servers = AGENT_CONFIG.get().unwrap().server.clone()
        .ok_or(NodegetError::ConfigNotFound("No server configuration found".to_owned()))?;
    rpc::multi_server::init_connections(servers);

    // 启动静态监控数据上报任务
    tokio::spawn(async {
        handle_static_monitoring_data_report().await;
    });

    // 启动动态监控数据上报任务
    tokio::spawn(async {
        handle_dynamic_monitoring_data_report().await;
    });

    // 启动错误消息处理任务
    tokio::spawn(async {
        handle_error_message().await;
    });

    // 启动任务处理任务
    tokio::spawn(async {
        handle_task().await;
    });

    // 等待 Ctrl+C 信号以优雅地关闭程序
    tokio::signal::ctrl_c().await.map_err(|e| NodegetError::Other(format!("Failed to listen for ctrl_c: {e}")))?;
    
    Ok(())
}
