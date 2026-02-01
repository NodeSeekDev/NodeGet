use crate::AGENT_CONFIG;
use crate::monitoring::impls::Monitor;
use crate::rpc::multi_server::{send_to, subscribe_to};
use crate::rpc::{wrap_json_into_rpc_with_id_1, JsonRpcErrorMessage, JsonRpcTask};
use log::{debug, error, info, trace, warn};
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use std::time::Duration;
use serde_json::Value;
use tokio::time;
use tokio::time::{MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use nodeget_lib::task::{TaskEventResponse, TaskEventResult, TaskEventType};
use nodeget_lib::utils::{get_local_timestamp_ms, JsonError};
use crate::tasks::ping;

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

pub async fn handle_error_message() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    for server in agent_config.server.clone().unwrap_or(vec![]) {
        tokio::spawn(async move {
            let mut rx = match subscribe_to(server.name.as_str()).await {
                Ok(rx) => {
                    rx
                }
                Err(e) => {
                    error!("[{}] Handle Error Message Error: {}", server.name, e);
                    return;
                }
            };

            while let Ok(message) = rx.recv().await {
                let server_name = server.name.clone();
                tokio::spawn(async move {
                    let rpc = match message {
                        Message::Text(text) => text.to_string(),
                        _ => {
                            return;
                        }
                    };

                    let json = match serde_json::from_str::<JsonRpcErrorMessage>(&rpc) {
                        Ok(json) => json,
                        Err(_) => {
                            return;
                        }
                    };

                    warn!("[{}] Received Error Message: {}: {}", server_name, json.result.error_id, json.result.error_message)
                });
            }
        });
    }
}
