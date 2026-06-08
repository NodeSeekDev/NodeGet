//! `agent.report_static` RPC 实现。
//!
//! Agent 上报静态监控数据。包含两级去重机制：
//! 1. **快速路径** — 内存 `StaticHashCache` 比较数据哈希
//! 2. **慢速路径** — 数据库查询哈希是否已存在（覆盖缓存初始化前的历史数据）
//!
//! 去重通过后，数据送入 `MonitoringBuffer` 批量写入，同时更新
//! `MonitoringLastCache` 和 `StaticHashCache`。

use crate::data_structure::StaticMonitoringData;
use crate::monitoring_buffer;
use crate::monitoring_last_cache::{build_static_value, MonitoringLastCache};
use crate::monitoring_uuid_cache::MonitoringUuidCache;
use crate::rpc::agent::AgentRpcImpl;
use crate::static_hash_cache::StaticHashCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{Permission, Scope, StaticMonitoring};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::static_monitoring;
use ng_infra::server::RpcHelper;
use ng_token::get::check_token_limit;
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use tracing::{debug, error, warn};

/// Agent 上报静态监控数据。
///
/// - `token` — 身份认证凭据
/// - `static_monitoring_data` — 静态监控数据
/// - 返回值 — `{"status": "buffered"}` 或 `{"status": "skipped", "reason": "duplicate_hash"}`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `StaticMonitoring::Write` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache::get_or_insert` 查找或创建 UUID→ID 映射
/// 3. 更新 `MonitoringLastCache` 内存缓存
/// 4. **快速去重**：检查 `StaticHashCache` 是否已有相同哈希
/// 5. **慢速去重**：查询数据库确认哈希是否已存在
/// 6. 通过去重后，构建 `ActiveModel` 送入 `MonitoringBuffer`
/// 7. 更新 `StaticHashCache` 哈希缓存
#[allow(clippy::too_many_lines)]
pub async fn report_static(
    token: String,
    static_monitoring_data: StaticMonitoringData,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        static CACHED: std::sync::OnceLock<Box<RawValue>> = std::sync::OnceLock::new();
        static CACHED_SKIPPED: std::sync::OnceLock<Box<RawValue>> = std::sync::OnceLock::new();
        let agent_uuid = static_monitoring_data.uuid;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: UUID parsed");

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(agent_uuid)],
            vec![Permission::StaticMonitoring(StaticMonitoring::Write)],
        )
        .await?;

        if !is_allowed {
            warn!(target: "monitoring", agent_uuid = %agent_uuid, "权限拒绝: 缺少 StaticMonitoring Write 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing StaticMonitoring Write permission for this Agent"
                    .to_string(),
            )
            .into());
        }
        debug!(target: "monitoring", agent_uuid = %agent_uuid, "report_static: permission check passed");

        let uuid_id = MonitoringUuidCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned()))?
            .get_or_insert(agent_uuid)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("UUID cache error: {e}")))?;

        let timestamp = static_monitoring_data.time.cast_signed();

        let cpu_val = serde_json::to_value(&static_monitoring_data.cpu)
            .map_err(|e| NodegetError::SerializationError(format!("cpu_data: {e}")))?;
        let system_val = serde_json::to_value(&static_monitoring_data.system)
            .map_err(|e| NodegetError::SerializationError(format!("system_data: {e}")))?;
        let gpu_val = serde_json::to_value(&static_monitoring_data.gpu)
            .map_err(|e| NodegetError::SerializationError(format!("gpu_data: {e}")))?;

        let cache_value = build_static_value(agent_uuid, timestamp, &static_monitoring_data);
        MonitoringLastCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringLastCache not initialized".to_owned()))?.update_static_prebuilt(agent_uuid, cache_value);

        // Fast path: check in-memory hash cache first to avoid DB query
        let hash_cache = StaticHashCache::global().ok_or_else(|| NodegetError::ConfigNotFound("StaticHashCache not initialized".to_owned()))?;
        if hash_cache.is_duplicate(uuid_id, &static_monitoring_data.data_hash) {
            debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data hash cached as duplicate, skipping");
            return Ok(CACHED_SKIPPED
                .get_or_init(|| RawValue::from_string(r#"{"status":"skipped","reason":"duplicate_hash"}"#.to_owned()).unwrap())
                .clone());
        }

        // Slow path: check DB for hash existence (covers hashes from before cache was populated)
        let db = <AgentRpcImpl as RpcHelper>::get_db()?;
        let exists = static_monitoring::Entity::find()
            .filter(static_monitoring::Column::UuidId.eq(uuid_id))
            .filter(static_monitoring::Column::DataHash.eq(static_monitoring_data.data_hash.as_slice()))
            .one(db)
            .await
            .map_err(|e| {
                error!(target: "monitoring", agent_uuid = %agent_uuid, error = %e, "report_static: DB hash check failed");
                NodegetError::DatabaseError(e.to_string())
            })?;

        if exists.is_some() {
            hash_cache.update(uuid_id, &static_monitoring_data.data_hash);
            debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data hash already exists, skipping");
            return Ok(CACHED_SKIPPED
                .get_or_init(|| RawValue::from_string(r#"{"status":"skipped","reason":"duplicate_hash"}"#.to_owned()).unwrap())
                .clone());
        }

        let data_hash = static_monitoring_data.data_hash;
        let in_data = static_monitoring::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(uuid_id),
            timestamp: Set(timestamp),
            storage_time: Set(Some(get_local_timestamp_ms_i64()?)),
            cpu_data: Set(cpu_val),
            system_data: Set(system_val),
            gpu_data: Set(gpu_val),
            data_hash: Set(data_hash.clone()),
        };

        debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Received static data, sending to buffer");

        monitoring_buffer::get().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringBuffers not initialized".to_owned()))?.static_mon.send(in_data);

        hash_cache.update(uuid_id, &data_hash);

        debug!(target: "monitoring", agent_uuid = %static_monitoring_data.uuid, "Static data buffered successfully");

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
