use crate::config_access::get_agent_config;
use crate::monitoring::impls::Monitor;
use crate::rpc::multi_server::send_to;
use log::{error, trace, warn};
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::monitoring::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

// 若 AGENT_CONFIG 尚未就绪，等待其可用后返回一份快照。
// 每次失败短暂 sleep 而不是 panic 退出任务，从而保证上报循环能在 reload/初始化瞬态后继续生效。
async fn wait_for_agent_config() -> AgentConfig {
    loop {
        match get_agent_config() {
            Ok(cfg) => return cfg,
            Err(e) => {
                warn!("Waiting for AGENT_CONFIG to become available: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// 把监控数据序列化成一份可在多个 server 上报任务中共享的 JSON 字符串。
///
/// 返回 `None` 表示序列化失败——由调用方自己决定是跳过本轮还是上报空值。
/// `Arc<str>` 的好处：
///   - `Arc::clone` 只是原子自增引用计数，O(1)；
///   - 下游 spawn 里拿到 `Arc<str>` 后直接 `&*arc` 拼 RPC 字符串，零额外分配。
/// 对比起以前"先 `to_value` → 每个 server clone 一次 Value（递归深拷贝 HashMap/Vec）"
/// 的做法，CPU 和 RSS `峰值都能显著降低（review_agent.md` #73）。
fn serialize_shared<T: Serialize>(data: &T) -> Option<Arc<str>> {
    match serde_json::to_string(data) {
        Ok(s) => Some(Arc::from(s.into_boxed_str())),
        Err(e) => {
            error!("Failed to serialize monitoring payload: {e}");
            None
        }
    }
}

/// 手工拼接一个 JSON-RPC 2.0 请求字符串，跳过 `serde_json::Value` 的中间态。
///
/// 调用方已经分别持有"token（原样字符串）"和"`data_json`（合法 JSON 片段）"，
/// 这里只需要按协议把它们拼进 params。token 会走 `serde_json::to_string` 以保证
/// 正确转义，`data_json` 原样嵌入（上游来自 `serialize_shared`，已是合法 JSON）。
fn build_rpc_with_raw_data(method: &str, token: &str, data_json: &str) -> String {
    // `serde_json::to_string(&str)` 对 String 绝不会失败；但保守起见兜底一个空串字面量。
    let token_json = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".to_owned());
    format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":[{token_json},{data_json}]}}"#)
}

// 处理静态监控数据上报
//
// 该函数按照配置的间隔时间刷新并获取静态监控数据（如 CPU、系统、GPU 基本信息），然后将其发送到配置的服务器
// 默认间隔时间为 5 分钟。
//
// 每次 tick 都会重新读取 AGENT_CONFIG，使运行时 reload 能立即影响 server 列表、token 以及
// 上报间隔。当 `static_report_interval_ms` 发生变化时会重建 ticker，让改动在下一轮立即生效。
pub async fn handle_static_monitoring_data_report() {
    let initial_config = wait_for_agent_config().await;
    let mut interval_ms = initial_config.static_report_interval_ms_or_default();
    let mut ticker = interval(Duration::from_millis(interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let agent_config = match get_agent_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                warn!("Skip static monitoring tick: {e}");
                continue;
            }
        };

        // Hot-reload: 若 interval 发生变化则重建 ticker
        let new_interval_ms = agent_config.static_report_interval_ms_or_default();
        if new_interval_ms != interval_ms {
            interval_ms = new_interval_ms;
            ticker = interval(Duration::from_millis(interval_ms));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            // 重建后跳过首次立即触发的 tick，避免同一轮内连续上报
            ticker.tick().await;
        }

        let static_monitoring_data = StaticMonitoringData::refresh_and_get().await;
        let Some(static_json) = serialize_shared(&static_monitoring_data) else {
            // 序列化失败时本轮彻底跳过——上报一个 null 既欺骗 server，也掩盖真实故障。
            continue;
        };

        trace!("Static Monitoring Data: {static_json}");

        for server in agent_config.server.unwrap_or_default() {
            let static_json = Arc::clone(&static_json);
            tokio::spawn(async move {
                let rpc =
                    build_rpc_with_raw_data("agent_report_static", &server.token, &static_json);
                if let Err(e) = send_to(&server.name, Message::Text(Utf8Bytes::from(rpc))).await {
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
//
// 与静态上报相同，每个 tick 都会重新读取 AGENT_CONFIG 以使 reload 生效。当 summary 间隔或
// dynamic 间隔发生变化时，会重建 ticker 并重置 `tick_count`。
pub async fn handle_dynamic_monitoring_data_report() {
    let initial_config = wait_for_agent_config().await;

    let mut dynamic_interval_ms = initial_config.dynamic_report_interval_ms_or_default();
    let mut summary_interval_ms = initial_config.dynamic_summary_report_interval_ms_or_default();

    // dynamic_interval_ms 是 summary_interval_ms 的整数倍（已在配置解析时校验）
    let mut ticks_per_dynamic = dynamic_interval_ms / summary_interval_ms;

    let mut ticker = interval(Duration::from_millis(summary_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut tick_count: u64 = 0;

    loop {
        ticker.tick().await;
        tick_count += 1;

        let agent_config = match get_agent_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                warn!("Skip dynamic monitoring tick: {e}");
                continue;
            }
        };

        // Hot-reload: summary 或 dynamic 间隔变化时重建 ticker
        let new_summary_ms = agent_config.dynamic_summary_report_interval_ms_or_default();
        let new_dynamic_ms = agent_config.dynamic_report_interval_ms_or_default();
        if new_summary_ms != summary_interval_ms || new_dynamic_ms != dynamic_interval_ms {
            summary_interval_ms = new_summary_ms;
            dynamic_interval_ms = new_dynamic_ms;
            ticks_per_dynamic = dynamic_interval_ms / summary_interval_ms;
            ticker = interval(Duration::from_millis(summary_interval_ms));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            tick_count = 0;
            ticker.tick().await;
        }

        let dynamic_monitoring_data = DynamicMonitoringData::refresh_and_get().await;

        // 每次 tick 都上报摘要数据
        let select_disk = agent_config.dynamic_summary_select_disk.as_deref();
        let select_nic = agent_config
            .dynamic_summary_select_network_interface
            .as_deref();
        let summary_data = DynamicMonitoringSummaryData::from_with_filter(
            &dynamic_monitoring_data,
            select_disk,
            select_nic,
        );
        if let Some(summary_json) = serialize_shared(&summary_data) {
            trace!("Dynamic Monitoring Summary Data: {summary_json}");

            for server in agent_config.server.clone().unwrap_or_default() {
                let summary_json = Arc::clone(&summary_json);
                tokio::spawn(async move {
                    let rpc = build_rpc_with_raw_data(
                        "agent_report_dynamic_summary",
                        &server.token,
                        &summary_json,
                    );
                    if let Err(e) = send_to(&server.name, Message::Text(Utf8Bytes::from(rpc))).await
                    {
                        error!("{e}");
                    }
                });
            }
        }

        // 当达到 dynamic 上报周期时，同时上报完整动态数据
        if tick_count >= ticks_per_dynamic {
            tick_count = 0;

            if let Some(dynamic_json) = serialize_shared(&dynamic_monitoring_data) {
                trace!("Dynamic Monitoring Data: {dynamic_json}");

                for server in agent_config.server.unwrap_or_default() {
                    let dynamic_json = Arc::clone(&dynamic_json);
                    tokio::spawn(async move {
                        let rpc = build_rpc_with_raw_data(
                            "agent_report_dynamic",
                            &server.token,
                            &dynamic_json,
                        );
                        if let Err(e) =
                            send_to(&server.name, Message::Text(Utf8Bytes::from(rpc))).await
                        {
                            error!("{e}");
                        }
                    });
                }
            }
        }
    }
}
