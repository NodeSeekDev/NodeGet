use crate::AGENT_CONFIG;
use crate::monitoring::impls::Monitor;
use crate::rpc::multi_server::send_to;
use crate::rpc::wrap_json_into_rpc_with_id_1;
use log::{error, trace};
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

// 处理静态监控数据上报
//
// 该函数每隔 1 分钟刷新并获取静态监控数据（如 CPU、系统、GPU 基本信息），然后将其发送到配置的服务器
pub async fn handle_static_monitoring_data_report() {
    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    let mut ticker = interval(Duration::from_mins(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let static_monitoring_data = StaticMonitoringData::refresh_and_get().await;
        let static_monitoring_data_value = serde_json::to_value(static_monitoring_data).unwrap();

        trace!("Static Monitoring Data: {static_monitoring_data_value}");

        for server in agent_config.server.clone().unwrap_or(vec![]) {
            let static_monitoring_data_value = static_monitoring_data_value.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to(
                    &server.name,
                    Message::Text(Utf8Bytes::from(wrap_json_into_rpc_with_id_1(
                        "agent_report_static",
                        vec![
                            serde_json::to_value(server.token).unwrap(),
                            static_monitoring_data_value,
                        ],
                    ))),
                )
                .await
                {
                    error!("{e}");
                }
            });
        }
    }
}

// 处理动态监控数据上报
//
// 该函数按照配置的间隔时间刷新并获取动态监控数据（如 CPU 使用率、内存、负载、网络等实时数据），然后将其发送到配置的服务器
// 默认间隔时间为 1 秒
pub async fn handle_dynamic_monitoring_data_report() {
    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");
    let interval_ms = agent_config.monitoring_report_interval_ms.unwrap_or(1000);

    let mut ticker = interval(Duration::from_millis(interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let dynamic_monitoring_data = DynamicMonitoringData::refresh_and_get().await;
        let dynamic_monitoring_data_value = serde_json::to_value(dynamic_monitoring_data).unwrap();

        trace!("Dynamic Monitoring Data: {dynamic_monitoring_data_value}");

        for server in agent_config.server.clone().unwrap_or(vec![]) {
            let dynamic_monitoring_data_value = dynamic_monitoring_data_value.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to(
                    &server.name,
                    Message::Text(Utf8Bytes::from(wrap_json_into_rpc_with_id_1(
                        "agent_report_dynamic",
                        vec![
                            serde_json::to_value(server.token).unwrap(),
                            dynamic_monitoring_data_value,
                        ],
                    ))),
                )
                .await
                {
                    error!("{e}");
                }
            });
        }
    }
}
