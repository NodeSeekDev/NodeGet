//! `crontab` RPC 权限校验：解析 CronType 并验证 Token 对应的 Scope/Permission。
//!
//! 提供写入权限检查（`ensure_crontab_payload_write_permission`）和
//! 通用 Scope 权限检查（`ensure_crontab_scope_permission`），
//! Agent 类型需额外检查 Task::Create 权限，
//! Server 类型需额外检查 JsWorker::RunDefinedJsWorker 权限。

use crate::{AgentCronType, CronType, ServerCronType};
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    Crontab as CrontabPermission, JsWorker as JsWorkerPermission, Permission, Scope, Task,
};
use ng_core::permission::permission_checker::require_permission_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use serde_json::Value;
use std::collections::HashSet;
use tracing::{trace, warn};

/// 从 CronType 提取所需的 Scope 列表。
/// Agent 类型映射为 AgentUuid Scope，Server 类型映射为 Global Scope。
/// 去重后返回。
fn scopes_from_cron_type(cron_type: &CronType) -> Vec<Scope> {
    let scopes = match cron_type {
        CronType::Agent(uuids, _) => uuids
            .iter()
            .map(|uuid| Scope::AgentUuid(*uuid))
            .collect::<Vec<_>>(),
        CronType::Server(_) => vec![Scope::Global],
    };

    // 去重：同一 UUID 在多条 Agent 配置中可能出现
    let deduped: HashSet<Scope> = scopes.into_iter().collect();
    deduped.into_iter().collect()
}

/// 从 CronType 提取写入操作所需的 Permission 列表。
/// 基础权限为 Crontab::Write，Agent 类型额外需要 Task::Create。
fn write_permissions_from_cron_type(cron_type: &CronType) -> Vec<Permission> {
    let mut permissions = vec![Permission::Crontab(CrontabPermission::Write)];

    if let CronType::Agent(_, AgentCronType::Task(task_event_type)) = cron_type {
        permissions.push(Permission::Task(Task::Create(
            task_event_type.task_name().to_owned(),
        )));
    }

    permissions
}

/// 从 JSON Value 解析 CronType，失败时返回包含任务名称的序列化错误。
///
/// - `cron_type_json` - 数据库中的 cron_type JSON 字段
/// - `name` - 定时任务名称，用于错误信息
pub fn parse_cron_type(cron_type_json: &Value, name: &str) -> anyhow::Result<CronType> {
    serde_json::from_value::<CronType>(cron_type_json.clone()).map_err(|e| {
        NodegetError::SerializationError(format!(
            "Failed to parse cron_type for crontab '{name}': {e}"
        ))
        .into()
    })
}

/// 检查 Crontab 负载写入权限（create/edit 操作使用）。
///
/// 1. 根据 CronType 提取 Scope 和 Permission
/// 2. Agent 类型：空 UUID 列表仅需 Global Scope 的 Crontab::Write；
///    非空列表需要覆盖所有 UUID Scope 的 Crontab::Write + Task::Create
/// 3. Server 类型：需要 Global Scope 的 Crontab::Write + JsWorker::RunDefinedJsWorker
///
/// - `token_or_auth` - Token 或用户名密码认证
/// - `cron_type` - 定时任务类型，决定所需权限
pub async fn ensure_crontab_payload_write_permission(
    token_or_auth: &TokenOrAuth,
    cron_type: &CronType,
) -> anyhow::Result<()> {
    trace!(target: "crontab", "checking crontab payload write permission");
    let checker = require_permission_checker()?;
    let scopes = scopes_from_cron_type(cron_type);
    let mut permissions = write_permissions_from_cron_type(cron_type);
    if matches!(cron_type, CronType::Agent(_, _)) {
        if scopes.is_empty() {
            // 空 Agent 列表：仅需 Global Scope 的 Crontab::Write
            let has_crontab_write = checker
                .check_token_limit(
                    token_or_auth,
                    vec![Scope::Global],
                    vec![Permission::Crontab(CrontabPermission::Write)],
                )
                .await?;
            if has_crontab_write {
                return Ok(());
            }
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing Crontab Write permission for empty agent list"
                    .to_owned(),
            )
            .into());
        }

        let is_allowed = checker.check_token_limit(token_or_auth, scopes, permissions).await?;
        if is_allowed {
            return Ok(());
        }

        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Insufficient Crontab/Task permissions for all target scopes"
                .to_owned(),
        )
        .into());
    }

    // Server 类型：仅保留 Crontab::Write，无需 Task::Create
    permissions.retain(|perm| matches!(perm, Permission::Crontab(CrontabPermission::Write)));
    let has_crontab_write = checker.check_token_limit(token_or_auth, scopes, permissions).await?;
    if !has_crontab_write {
        warn!(target: "crontab", "crontab write permission denied in global scope");
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Missing crontab write permission in global scope".to_owned(),
        )
        .into());
    }

    // Server 类型额外检查 JsWorker 运行权限
    if let CronType::Server(ServerCronType::JsWorker(worker_name, _)) = cron_type {
        if worker_name.trim().is_empty() {
            return Err(NodegetError::InvalidInput(
                "Invalid crontab payload: js worker name cannot be empty".to_owned(),
            )
            .into());
        }

        let has_js_worker_run = checker
            .check_token_limit(
                token_or_auth,
                vec![Scope::JsWorker(worker_name.clone())],
                vec![Permission::JsWorker(JsWorkerPermission::RunDefinedJsWorker)],
            )
            .await?;

        if !has_js_worker_run {
            warn!(target: "crontab", worker_name = %worker_name, "missing js_worker run permission for crontab server JsWorker type");
            return Err(NodegetError::PermissionDenied(format!(
                "Permission Denied: Missing js_worker run permission for '{worker_name}'"
            ))
            .into());
        }
    }

    Ok(())
}

/// 检查 Crontab Scope 权限（delete/set_enable 操作使用）。
///
/// 根据 CronType 提取 Scope 列表，验证 Token 是否在所有目标 Scope 上
/// 拥有指定 Permission。空 Scope 列表回退到 Global Scope。
///
/// - `token_or_auth` - Token 或用户名密码认证
/// - `cron_type` - 定时任务类型，决定所需 Scope
/// - `permission` - 待检查的权限（如 Crontab::Delete、Crontab::Write）
/// - `denied_message` - 权限不足时的错误消息
pub async fn ensure_crontab_scope_permission(
    token_or_auth: &TokenOrAuth,
    cron_type: &CronType,
    permission: Permission,
    denied_message: &'static str,
) -> anyhow::Result<()> {
    trace!(target: "crontab", "checking crontab scope permission");
    let scopes = scopes_from_cron_type(cron_type);
    // 空 Scope 列表回退到 Global Scope
    let scopes = if scopes.is_empty() {
        vec![Scope::Global]
    } else {
        scopes
    };
    let checker = require_permission_checker()?;
    let is_allowed = checker.check_token_limit(token_or_auth, scopes, vec![permission]).await?;

    if is_allowed {
        Ok(())
    } else {
        warn!(target: "crontab", "crontab scope permission denied");
        Err(NodegetError::PermissionDenied(denied_message.to_owned()).into())
    }
}
