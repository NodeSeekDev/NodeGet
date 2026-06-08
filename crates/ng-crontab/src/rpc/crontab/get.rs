//! `crontab.get` RPC 实现：获取定时任务列表。
//!
//! Super-token 返回全部条目；普通 Token 按权限过滤：
//! - Global Scope 可见所有条目
//! - AgentUuid Scope 仅可见包含该 UUID 的 Agent 类型任务
//! - Server 类型任务仅 Global Scope 可见

use crate::CronType;
use crate::cache::CrontabCache;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::data_structure::{Crontab as CrontabPermission, Permission, Scope, Token};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_token::{check_super_token, get_token};
use serde_json::value::RawValue;
use std::collections::HashSet;
use tracing::{debug, warn};
use uuid::Uuid;

/// 获取定时任务列表。
///
/// 1. 解析 Token 格式
/// 2. 检查是否为 Super-token（是则返回全部条目）
/// 3. 普通 Token：验证有效期、检查 Crontab::Read 权限
/// 4. 根据 Token 的 Scope 过滤可见条目
///
/// - `token` - 认证 Token 字符串
/// - 返回 `Vec<Cron>` 的 JSON 序列化
pub async fn get(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab", "processing crontab get request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        let cache = CrontabCache::global().ok_or_else(|| {
            NodegetError::ConfigNotFound("CrontabCache not initialized".to_owned())
        })?;
        let entries = cache.get_all_entries();

        // Super-token 直接返回全部条目，无需权限过滤
        if is_super_token {
            let crontabs: Vec<crate::Cron> = entries
                .into_iter()
                .map(|entry| crate::Cron {
                    id: entry.model.id,
                    name: entry.model.name.clone(),
                    enable: entry.model.enable,
                    cron_expression: entry.model.cron_expression.clone(),
                    cron_type: entry.cron_type.clone(),
                    last_run_time: cache
                        .get_last_run_time(entry.model.id, entry.model.last_run_time),
                })
                .collect();

            return serde_json::value::to_raw_value(&crontabs)
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
        }

        // 普通 Token：获取 Token 信息并验证有效期
        let token_info = get_token(&token_or_auth).await?;

        let now = get_local_timestamp_ms_i64()?;

        // 检查 Token 时间有效性
        if let Some(from) = token_info.timestamp_from
            && now < from
        {
            return Err(NodegetError::PermissionDenied("Token is not yet valid".to_owned()).into());
        }

        if let Some(to) = token_info.timestamp_to
            && now > to
        {
            return Err(NodegetError::PermissionDenied("Token has expired".to_owned()).into());
        }

        // 检查是否至少有一个 limit 包含 Crontab::Read 权限
        let has_crontab_read_permission = token_info.token_limit.iter().any(|limit| {
            limit
                .permissions
                .iter()
                .any(|perm| matches!(perm, Permission::Crontab(CrontabPermission::Read)))
        });

        if !has_crontab_read_permission {
            warn!(target: "crontab", "crontab read permission denied");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient Crontab Read permission".to_owned(),
            )
            .into());
        }

        // 根据 Token 的 Scope 过滤可见条目
        let crontabs = filter_entries_by_token(&entries, &token_info, cache);
        serde_json::value::to_raw_value(&crontabs)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

/// 根据 Token 的 Scope 和 Permission 过滤可见的定时任务条目。
///
/// - Global Scope：可见所有条目
/// - AgentUuid Scope：仅可见包含该 UUID 的 Agent 类型任务
/// - Server 类型任务：仅 Global Scope 可见
fn filter_entries_by_token(
    entries: &[std::sync::Arc<crate::cache::CachedCrontab>],
    token_info: &Token,
    cache: &CrontabCache,
) -> Vec<crate::Cron> {
    let mut has_global = false;
    let mut allowed_uuids: HashSet<Uuid> = HashSet::new();

    // 遍历 Token 的所有限制，收集允许的 Scope

    for limit in &token_info.token_limit {
        let has_crontab_read = limit
            .permissions
            .iter()
            .any(|p| matches!(p, Permission::Crontab(CrontabPermission::Read)));

        if !has_crontab_read {
            continue;
        }

        for scope in &limit.scopes {
            match scope {
                Scope::Global => {
                    has_global = true;
                }
                Scope::AgentUuid(uuid) => {
                    allowed_uuids.insert(*uuid);
                }
                // 这些 Scope 不适用于 crontab 权限检查，忽略
                Scope::KvNamespace(_)
                | Scope::JsWorker(_)
                | Scope::StaticBucket(_)
                | Scope::Db(_) => {}
            }
        }
    }

    entries
        .iter()
        .filter(|entry| {
            if has_global {
                return true;
            }
            match &entry.cron_type {
                CronType::Agent(agent_uuids, _) => {
                    agent_uuids.iter().any(|uuid| allowed_uuids.contains(uuid))
                }
                CronType::Server(_) => false,
            }
        })
        .map(|entry| crate::Cron {
            id: entry.model.id,
            name: entry.model.name.clone(),
            enable: entry.model.enable,
            cron_expression: entry.model.cron_expression.clone(),
            cron_type: entry.cron_type.clone(),
            last_run_time: cache.get_last_run_time(entry.model.id, entry.model.last_run_time),
        })
        .collect()
}
