//! `nodeget-server.list_all_agent_uuid` RPC 实现。
//!
//! 列出所有 Agent UUID，根据 Token 权限过滤可见范围。
//! 权限判定逻辑：
//! - Super-token → 可见所有 UUID
//! - 拥有 `ListAllAgentUuid` 全局权限 → 可见所有 UUID
//! - 拥有 `ListAllAgentUuid` `AgentUuid` 级权限 + 至少一种非 List 操作权限 → 可见对应 UUID
//! - 其他 → 拒绝访问

use crate::monitoring_uuid_cache::MonitoringUuidCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{MonitoringUuid, NodeGet, Permission, Scope};
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_token::{TokenOrAuth, check_super_token, get_token};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, trace, warn};
use uuid::Uuid;

/// Agent UUID 列表权限等级。
enum AgentUuidListPermission {
    /// 可见所有 UUID
    All,
    /// 仅可见指定集合的 UUID
    Scoped(HashSet<Uuid>),
}

/// 列出 Agent UUID 的响应结构体。
#[derive(Serialize)]
struct ListAllAgentUuidResponse {
    /// 可见的 UUID 列表
    uuids: Vec<Uuid>,
}

/// 列出所有 Agent UUID，根据 Token 权限过滤可见范围。
///
/// - `token` — 身份认证凭据
/// - 返回值 — `{"uuids": [...]}` 格式的 JSON
///
/// 内部步骤：
/// 1. 解析 Token 并判断权限等级
/// 2. 从 `MonitoringUuidCache` 获取所有活跃 UUID
/// 3. 根据权限等级过滤可见 UUID
/// 4. 序列化返回
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
/// - 序列化失败时返回 `NodegetError::SerializationError`
pub async fn list_all_agent_uuid(token: String) -> RpcResult<Box<RawValue>> {
    debug!(target: "server", "listing all agent uuids");
    let process_logic = async {
        let permission = resolve_list_agent_uuid_permission(&token).await?;

        let all_uuids = MonitoringUuidCache::global()
            .ok_or_else(|| {
                NodegetError::ConfigNotFound("MonitoringUuidCache not initialized".to_owned())
            })?
            .list_all();
        let uuids = match permission {
            AgentUuidListPermission::All => all_uuids,
            AgentUuidListPermission::Scoped(allowed) => all_uuids
                .into_iter()
                .filter(|uuid| allowed.contains(uuid))
                .collect(),
        };

        let response = ListAllAgentUuidResponse { uuids };
        debug!(target: "server", uuid_count = response.uuids.len(), "list_all_agent_uuid completed");
        serde_json::value::to_raw_value(&response)
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

/// 解析 Token 对 Agent UUID 列表的访问权限。
///
/// 内部步骤：
/// 1. 解析 Token 为 `TokenOrAuth`
/// 2. 检查是否为 Super-token → `All`
/// 3. 获取 Token 信息并检查有效期
/// 4. 遍历 Token 的 limit 列表，收集 List 权限和操作权限的 Scope
/// 5. 拥有全局 List 权限 → `All`
/// 6. 拥有 `AgentUuid` 级 List 权限 + 至少一种操作权限 → `Scoped`
/// 7. 无任何 List 权限 → 返回权限拒绝错误
async fn resolve_list_agent_uuid_permission(
    token: &str,
) -> anyhow::Result<AgentUuidListPermission> {
    trace!(target: "server", "resolving agent uuid list permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_super_token = check_super_token(&token_or_auth).await.map_err(|e| {
        warn!(target: "monitoring", "权限拒绝: super token 校验失败");
        NodegetError::PermissionDenied(format!("{e}"))
    })?;
    if is_super_token {
        trace!(target: "server", "Super token detected, granting All agent UUID access");
        return Ok(AgentUuidListPermission::All);
    }

    let token_info = get_token(&token_or_auth).await?;
    let now = get_local_timestamp_ms_i64()?;

    if let Some(from) = token_info.timestamp_from
        && now < from
    {
        warn!(target: "monitoring", "权限拒绝: Token 尚未生效");
        return Err(NodegetError::PermissionDenied("Token is not yet valid".to_owned()).into());
    }

    if let Some(to) = token_info.timestamp_to
        && now > to
    {
        warn!(target: "monitoring", "权限拒绝: Token 已过期");
        return Err(NodegetError::PermissionDenied("Token has expired".to_owned()).into());
    }

    let mut has_global_list_permission = false;
    let mut nodeget_scoped_uuids: HashSet<Uuid> = HashSet::new();
    let mut operable_scoped_uuids: HashSet<Uuid> = HashSet::new();

    for limit in token_info.token_limit.iter() {
        #[allow(deprecated)]
        let has_list_permission = limit.permissions.iter().any(|perm| {
            matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid))
                || matches!(perm, Permission::MonitoringUuid(MonitoringUuid::List))
        });

        if has_list_permission {
            if limit
                .scopes
                .iter()
                .any(|scope| matches!(scope, Scope::Global))
            {
                has_global_list_permission = true;
            }

            for scope in &limit.scopes {
                if let Scope::AgentUuid(uuid) = scope {
                    nodeget_scoped_uuids.insert(*uuid);
                }
            }
        }

        // "可操作" = 对该 AgentUuid Scope 至少拥有一种非 list 权限
        #[allow(deprecated)]
        let has_any_operation_permission = limit.permissions.iter().any(|perm| {
            !matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid))
                && !matches!(perm, Permission::MonitoringUuid(MonitoringUuid::List))
        });

        if has_any_operation_permission {
            for scope in &limit.scopes {
                if let Scope::AgentUuid(uuid) = scope {
                    operable_scoped_uuids.insert(*uuid);
                }
            }
        }
    }

    if has_global_list_permission {
        trace!(target: "server", "Global ListAllAgentUuid permission found, granting All access");
        return Ok(AgentUuidListPermission::All);
    }

    if nodeget_scoped_uuids.is_empty() {
        warn!(target: "monitoring", "权限拒绝: 缺少 ListAllAgentUuid 权限");
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Insufficient NodeGet ListAllAgentUuid permissions".to_owned(),
        )
        .into());
    }

    let allowed_scoped_uuids: HashSet<Uuid> = nodeget_scoped_uuids
        .into_iter()
        .filter(|uuid| operable_scoped_uuids.contains(uuid))
        .collect();

    trace!(target: "server", allowed_count = allowed_scoped_uuids.len(), "Scoped agent UUID permission resolved");
    Ok(AgentUuidListPermission::Scoped(allowed_scoped_uuids))
}
