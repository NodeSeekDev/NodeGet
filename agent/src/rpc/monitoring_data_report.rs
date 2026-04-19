use crate::AGENT_CONFIG;
use crate::monitoring::impls::Monitor;
use crate::rpc::multi_server::send_to;
use crate::rpc::wrap_json_into_rpc_with_id_1;
use log::{error, trace};
use nodeget_lib::monitoring::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

// 处理静态监控数据上报
//
// 该函数按照配置的间隔时间刷新并获取静态监控数据（如 CPU、系统、GPU 基本信息），然后将其发送到配置的服务器
// 默认间隔时间为 5 分钟
pub async fn handle_static_monitoring_data_report() {
    let agent_config = AGENT_CONFIG
        .get()
        .expect("Agent config not initialized")
        .read()
        .expect("AGENT_CONFIG lock poisoned")
        .clone();

    let interval_ms = agent_config.static_report_interval_ms.unwrap_or(300_000);
    let mut ticker = interval(Duration::from_millis(interval_ms));
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

// 处理动态监控数据及摘要数据上报
//
// 该函数同时处理动态监控数据和动态监控摘要数据的上报。
// 以 summary 间隔为基础 tick，每次 tick 采集一次 DynamicMonitoringData 并提取摘要上报。
// 当累计 tick 次数达到 dynamic_interval / summary_interval 时，同时上报完整的动态监控数据。
// 默认两个间隔均为 1 秒。
pub async fn handle_dynamic_monitoring_data_report() {
    let agent_config = AGENT_CONFIG
        .get()
        .expect("Agent config not initialized")
        .read()
        .expect("AGENT_CONFIG lock poisoned")
        .clone();

    let dynamic_interval_ms = agent_config.dynamic_report_interval_ms.unwrap_or(1000);
    let summary_interval_ms = agent_config
        .dynamic_summary_report_interval_ms
        .unwrap_or(1000);

    // dynamic_interval_ms 是 summary_interval_ms 的整数倍（已在配置解析时校验）
    let ticks_per_dynamic = (dynamic_interval_ms / summary_interval_ms) as u64;

    let mut ticker = interval(Duration::from_millis(summary_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut tick_count: u64 = 0;

    loop {
        ticker.tick().await;
        tick_count += 1;

        let dynamic_monitoring_data = DynamicMonitoringData::refresh_and_get().await;

        // 每次 tick 都上报摘要数据
        let summary_data = DynamicMonitoringSummaryData::from(&dynamic_monitoring_data);
        let summary_value = serde_json::to_value(&summary_data).unwrap();
        trace!("Dynamic Monitoring Summary Data: {summary_value}");

        for server in agent_config.server.clone().unwrap_or(vec![]) {
            let summary_value = summary_value.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to(
                    &server.name,
                    Message::Text(Utf8Bytes::from(wrap_json_into_rpc_with_id_1(
                        "agent_report_dynamic_summary",
                        vec![serde_json::to_value(server.token).unwrap(), summary_value],
                    ))),
                )
                .await
                {
                    error!("{e}");
                }
            });
        }

        // 当达到 dynamic 上报周期时，同时上报完整动态数据
        if tick_count >= ticks_per_dynamic {
            tick_count = 0;

            let dynamic_value = serde_json::to_value(&dynamic_monitoring_data).unwrap();
            trace!("Dynamic Monitoring Data: {dynamic_value}");

            for server in agent_config.server.clone().unwrap_or(vec![]) {
                let dynamic_value = dynamic_value.clone();
                tokio::spawn(async move {
                    if let Err(e) = send_to(
                        &server.name,
                        Message::Text(Utf8Bytes::from(wrap_json_into_rpc_with_id_1(
                            "agent_report_dynamic",
                            vec![serde_json::to_value(server.token).unwrap(), dynamic_value],
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
}
