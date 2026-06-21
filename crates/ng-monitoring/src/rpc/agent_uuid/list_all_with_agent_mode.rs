//! `agent-uuid.list_all_with_agent_mode` RPC 实现。
//!
//! 列出所有 Agent UUID（包含软删除状态），需要 `MonitoringUuid::List` 权限。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_token::get::check_token_limit;
use serde_json::value::RawValue;
use tracing::{debug, warn};

/// 响应项，包含 UUID 及其软删除状态。
#[derive(serde::Serialize)]
struct AgentUuidWithMode {
    /// 设备 UUID
    uuid: uuid::Uuid,
    /// 是否已软删除
    soft_delete: bool,
}

/// 列出所有 Agent UUID 及其软删除状态。
///
/// - `token` — 身份认证凭据
/// - 返回值 — `AgentUuidWithMode` 数组的 JSON 序列化
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `MonitoringUuid::List` 权限
/// 2. 从 `MonitoringUuidCache` 获取所有 UUID 及其状态
/// 3. 转换为 `AgentUuidWithMode` 列表并序列化返回
pub async fn list_all_agent_uuids_with_agent_mode(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", "list_all_agent_uuids_with_agent_mode: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            &[Scope::Global],
            &[Permission::MonitoringUuid(MonitoringUuid::List)],
        )
        .await?;

        if !is_allowed {
            warn!(target: "monitoring", "权限拒绝: 缺少 MonitoringUuid::List 权限");
            return Err(anyhow::anyhow!(NodegetError::PermissionDenied(
                "Permission Denied: Missing MonitoringUuid::List permission".to_owned(),
            )));
        }
        debug!(target: "rpc", "list_all_agent_uuids_with_agent_mode: permission check passed");

        let uuids = MonitoringUuidCache::global()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
            })?
            .list_all_with_agent_mode();

        let items: Vec<AgentUuidWithMode> = uuids
            .into_iter()
            .map(|(uuid, soft_delete)| AgentUuidWithMode { uuid, soft_delete })
            .collect();

        serde_json::value::to_raw_value(&items)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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
