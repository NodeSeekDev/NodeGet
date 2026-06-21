//! `agent.report_dynamic` RPC 实现。
//!
//! Agent 上报动态监控数据。数据经权限校验后同时送入：
//! 1. `MonitoringBuffer` — 异步批量写入数据库
//! 2. `MonitoringLastCache` — 更新内存中的最新值缓存

use crate::data_structure::DynamicMonitoringData;
use crate::monitoring_buffer;
use crate::monitoring_last_cache::{MonitoringLastCache, build_dynamic_value_prebuilt};
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::dynamic_monitoring;
use ng_token::get::check_token_limit;
use sea_orm::{ActiveValue, Set};
use serde_json::value::RawValue;
use tracing::debug;

/// Agent 上报动态监控数据。
///
/// - `token` — 身份认证凭据
/// - `dynamic_monitoring_data` — 动态监控数据
/// - 返回值 — `{"status": "buffered"}`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Write` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache::get_or_insert` 查找或创建 UUID→ID 映射
/// 3. 将各字段序列化为 JSON 并构建 `ActiveModel`
/// 4. 送入 `MonitoringBuffer` 等待批量写入
/// 5. 同时更新 `MonitoringLastCache` 内存缓存
pub async fn report_dynamic(
    token: String,
    dynamic_monitoring_data: DynamicMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        static CACHED: std::sync::OnceLock<Box<RawValue>> = std::sync::OnceLock::new();
        let agent_uuid = dynamic_monitoring_data.uuid;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            &[Scope::AgentUuid(agent_uuid)],
            &[Permission::DynamicMonitoring(DynamicMonitoring::Write)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Write permission for this Agent"
                    .to_owned(),
            )
            .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_dynamic: permission check passed");

        let uuid_id = MonitoringUuidCache::global()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
            })?
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let timestamp = dynamic_monitoring_data.time.cast_signed();

        let cpu_val = serde_json::to_value(&dynamic_monitoring_data.cpu)
            .map_err(|e| NodegetError::SerializationError(format!("cpu_data: {e}")))?;
        let ram_val = serde_json::to_value(&dynamic_monitoring_data.ram)
            .map_err(|e| NodegetError::SerializationError(format!("ram_data: {e}")))?;
        let load_val = serde_json::to_value(&dynamic_monitoring_data.load)
            .map_err(|e| NodegetError::SerializationError(format!("load_data: {e}")))?;
        let system_val = serde_json::to_value(&dynamic_monitoring_data.system)
            .map_err(|e| NodegetError::SerializationError(format!("system_data: {e}")))?;
        let disk_val = serde_json::to_value(&dynamic_monitoring_data.disk)
            .map_err(|e| NodegetError::SerializationError(format!("disk_data: {e}")))?;
        let network_val = serde_json::to_value(&dynamic_monitoring_data.network)
            .map_err(|e| NodegetError::SerializationError(format!("network_data: {e}")))?;
        let gpu_val = serde_json::to_value(&dynamic_monitoring_data.gpu)
            .map_err(|e| NodegetError::SerializationError(format!("gpu_data: {e}")))?;

        let in_data = dynamic_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(timestamp),
            storage_time: Set(Some(get_local_timestamp_ms_i64()?)),
            // ActiveModel 各字段 clone（Value 结构复制，远比 to_value 反射序列化廉价），
            // 原值 move 给 cache_value，避免 build_dynamic_value 重新 to_value 7 次。
            cpu_data: Set(cpu_val.clone()),
            ram_data: Set(ram_val.clone()),
            load_data: Set(load_val.clone()),
            system_data: Set(system_val.clone()),
            disk_data: Set(disk_val.clone()),
            network_data: Set(network_val.clone()),
            gpu_data: Set(gpu_val.clone()),
        };

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Received dynamic data, sending to buffer");

        monitoring_buffer::get()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringBuffers not initialized".to_owned())
            })?
            .dynamic_mon
            .send(in_data);

        let cache_value = build_dynamic_value_prebuilt(
            agent_uuid,
            timestamp,
            cpu_val,
            ram_val,
            load_val,
            system_val,
            disk_val,
            network_val,
            gpu_val,
        );
        MonitoringLastCache::global()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringLastCache not initialized".to_owned())
            })?
            .update_dynamic_prebuilt(agent_uuid, cache_value);

        debug!(target: "monitoring", agent_uuid = %dynamic_monitoring_data.uuid, "Dynamic data buffered successfully");

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
