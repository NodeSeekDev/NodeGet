//! `agent.report_dynamic_summary` RPC 实现。
//!
//! Agent 上报动态摘要监控数据。数据经权限校验后同时送入：
//! 1. `MonitoringBuffer` — 异步批量写入数据库
//! 2. `MonitoringLastCache` — 更新内存中的最新值缓存

use crate::data_structure::DynamicMonitoringSummaryData;
use crate::monitoring_buffer;
use crate::monitoring_last_cache::MonitoringLastCache;
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoringSummary, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::dynamic_monitoring_summary;
use ng_token::get::check_token_limit;
use sea_orm::{ActiveValue, Set};
use serde_json::value::RawValue;
use tracing::debug;

/// Agent 上报动态摘要监控数据。
///
/// - `token` — 身份认证凭据
/// - `data` — 动态摘要监控数据
/// - 返回值 — `{"status": "buffered"}`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoringSummary::Write` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache::get_or_insert` 查找或创建 UUID→ID 映射
/// 3. 构建摘要数据的 `ActiveModel`
/// 4. 送入 `MonitoringBuffer` 等待批量写入
/// 5. 同时更新 `MonitoringLastCache` 内存缓存
pub async fn report_dynamic_summary(
    token: String,
    data: DynamicMonitoringSummaryData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        static CACHED: std::sync::OnceLock<Box<RawValue>> = std::sync::OnceLock::new();
        let agent_uuid = data.uuid;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::DynamicMonitoringSummary(
                DynamicMonitoringSummary::Write,
            )],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoringSummary Write permission for this Agent"
                    .to_owned(),
            )
                .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic_summary: permission check passed");

        let uuid_id = MonitoringUuidCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned()))?
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let in_data = dynamic_monitoring_summary::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(data.time.cast_signed()),
            storage_time: Set(Some(get_local_timestamp_ms_i64()?)),
            cpu_usage: Set(data.cpu_usage),
            gpu_usage: Set(data.gpu_usage),
            used_swap: Set(data.used_swap),
            total_swap: Set(data.total_swap),
            used_memory: Set(data.used_memory),
            total_memory: Set(data.total_memory),
            available_memory: Set(data.available_memory),
            load_one: Set(data.load_one),
            load_five: Set(data.load_five),
            load_fifteen: Set(data.load_fifteen),
            uptime: Set(data.uptime),
            boot_time: Set(data.boot_time),
            process_count: Set(data.process_count),
            total_space: Set(data.total_space),
            available_space: Set(data.available_space),
            read_speed: Set(data.read_speed),
            write_speed: Set(data.write_speed),
            tcp_connections: Set(data.tcp_connections),
            udp_connections: Set(data.udp_connections),
            total_received: Set(data.total_received),
            total_transmitted: Set(data.total_transmitted),
            transmit_speed: Set(data.transmit_speed),
            receive_speed: Set(data.receive_speed),
        };

        debug!(target: "monitoring", agent_uuid = %data.uuid, "Received dynamic summary data, sending to buffer");

        monitoring_buffer::get().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringBuffers not initialized".to_owned()))?.dynamic_summary.send(in_data);

        MonitoringLastCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringLastCache not initialized".to_owned()))?.update_dynamic_summary_prebuilt(
            agent_uuid,
            crate::monitoring_last_cache::build_dynamic_summary_value(
                agent_uuid,
                data.time.cast_signed(),
                &data,
            ),
        );

        debug!(target: "monitoring", agent_uuid = %data.uuid, "Dynamic summary data buffered successfully");

        Ok(CACHED
            .get_or_init(|| RawValue::from_string(r#"{"status":"buffered"}"#.to_owned()).unwrap())
            .clone())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
