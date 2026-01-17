use crate::AGENT_CONFIG;
use crate::monitoring::impls::Monitor;
use crate::rpc::multi_server::send_to;
use crate::rpc::wrap_json_into_rpc_with_id_1;
use log::{error, trace};
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

pub async fn handle_static_monitoring_data_report() {
    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    loop {
        let static_monitoring_data = StaticMonitoringData::refresh_and_get().await;
        let static_monitoring_data_value = serde_json::to_value(static_monitoring_data).unwrap();

        trace!("Static Monitoring Data: {}", static_monitoring_data_value);

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

        tokio::time::sleep(Duration::from_mins(5)).await;
    }
}

pub async fn handle_dynamic_monitoring_data_report() {
    let agent_config = AGENT_CONFIG.get().expect("Agent config not initialized");

    loop {
        let dynamic_monitoring_data = DynamicMonitoringData::refresh_and_get().await;
        let dynamic_monitoring_data_value = serde_json::to_value(dynamic_monitoring_data).unwrap();

        trace!("Dynamic Monitoring Data: {}", dynamic_monitoring_data_value);

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

        tokio::time::sleep(Duration::from_millis(
            agent_config.monitoring_report_interval_ms.unwrap_or(1000),
        ))
        .await;
    }
}
