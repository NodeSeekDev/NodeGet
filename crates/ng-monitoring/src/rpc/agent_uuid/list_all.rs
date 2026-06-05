//! `agent-uuid.list_all` RPC 实现。
//!
//! 列出所有非软删除的 Agent UUID，需要 `MonitoringUuid::List` 权限。

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_token::get::check_token_limit;
use serde_json::value::RawValue;
use tracing::debug;

/// 列出所有非软删除的 Agent UUID。
///
/// - `token` — 身份认证凭据
/// - 返回值 — UUID 数组的 JSON 序列化
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `MonitoringUuid::List` 权限
/// 2. 从 `MonitoringUuidCache` 获取所有活跃 UUID
/// 3. 序列化返回
pub async fn list_all_agent_uuids(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "rpc", "list_all_agent_uuids: token parsed");

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::MonitoringUuid(MonitoringUuid::List)],
        )
        .await?;

        if !is_allowed {
            return Err(anyhow::anyhow!(NodegetError::PermissionDenied(
                "Permission Denied: Missing MonitoringUuid::List permission".to_owned(),
            )));
        }
        debug!(target: "rpc", "list_all_agent_uuids: permission check passed");

        let uuids = MonitoringUuidCache::global().list_all();

        serde_json::value::to_raw_value(&uuids)
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
