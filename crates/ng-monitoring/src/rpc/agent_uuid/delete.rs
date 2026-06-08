//! `agent-uuid.delete` RPC 实现。
//!
//! 按 UUID 软删除 Agent 记录，需要 `MonitoringUuid::Delete` 权限。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_token::get::check_token_limit;
use serde_json::value::RawValue;
use tracing::{debug, warn};
use uuid::Uuid;

/// 软删除指定 Agent UUID。
///
/// - `token` — 身份认证凭据
/// - `agent_uuid` — 待软删除的 Agent UUID
/// - 返回值 — `{"success": true/false, "message": "..."}`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `MonitoringUuid::Delete` 权限
/// 2. 调用 `MonitoringUuidCache::soft_delete()` 执行软删除
/// 3. 返回操作结果
pub async fn delete_agent_uuid(token: String, agent_uuid: Uuid) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", %agent_uuid, "delete_agent_uuid: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::MonitoringUuid(MonitoringUuid::Delete)],
        )
        .await?;

        if !is_allowed {
            warn!(target: "monitoring", %agent_uuid, "权限拒绝: 缺少 MonitoringUuid::Delete 权限");
            return Err(anyhow::anyhow!(NodegetError::PermissionDenied(
                "Permission Denied: Missing MonitoringUuid::Delete permission".to_owned(),
            )));
        }
        debug!(target: "rpc", %agent_uuid, "delete_agent_uuid: permission check passed");

        let deleted = MonitoringUuidCache::global().ok_or_else(|| NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned()))?
            .soft_delete(agent_uuid)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to soft delete agent UUID: {e}"))
            })?;

        let raw = if deleted {
            serde_json::value::to_raw_value(&serde_json::json!({
                "success": true,
                "message": "Agent UUID soft-deleted"
            }))
        } else {
            serde_json::value::to_raw_value(&serde_json::json!({
                "success": false,
                "message": "Agent UUID not found"
            }))
        };
        raw.map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
