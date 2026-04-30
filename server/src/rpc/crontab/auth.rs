use crate::token::get::check_token_limit;
use nodeget_lib::crontab::{AgentCronType, CronType, ServerCronType};
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{
    Crontab as CrontabPermission, JsWorker as JsWorkerPermission, Permission, Scope, Task,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::Value;
use tracing::{trace, warn};

fn scopes_from_cron_type(cron_type: &CronType) -> Vec<Scope> {
    let scopes = match cron_type {
        CronType::Agent(uuids, _) => uuids
            .iter()
            .map(|uuid| Scope::AgentUuid(*uuid))
            .collect::<Vec<_>>(),
        CronType::Server(_) => vec![Scope::Global],
    };

    let mut deduped = Vec::with_capacity(scopes.len());
    for scope in scopes {
        if !deduped.contains(&scope) {
            deduped.push(scope);
        }
    }

    deduped
}

fn write_permissions_from_cron_type(cron_type: &CronType) -> Vec<Permission> {
    let mut permissions = vec![Permission::Crontab(CrontabPermission::Write)];

    if let CronType::Agent(_, AgentCronType::Task(task_event_type)) = cron_type {
        permissions.push(Permission::Task(Task::Create(
            task_event_type.task_name().to_owned(),
        )));
    }

    permissions
}

pub fn parse_cron_type(cron_type_json: &Value, name: &str) -> anyhow::Result<CronType> {
    serde_json::from_value::<CronType>(cron_type_json.clone()).map_err(|e| {
        NodegetError::SerializationError(format!(
            "Failed to parse cron_type for crontab '{name}': {e}"
        ))
        .into()
    })
}

pub async fn ensure_crontab_payload_write_permission(
    token_or_auth: &TokenOrAuth,
    cron_type: &CronType,
) -> anyhow::Result<()> {
    trace!(target: "crontab", "checking crontab payload write permission");
    let scopes = scopes_from_cron_type(cron_type);
    let mut permissions = write_permissions_from_cron_type(cron_type);
    if matches!(cron_type, CronType::Agent(_, _)) {
        if scopes.is_empty() {
            // Empty agent list: only require Crontab::Write on Global scope
            let has_crontab_write = check_token_limit(
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

        let is_allowed = check_token_limit(token_or_auth, scopes, permissions).await?;
        if is_allowed {
            return Ok(());
        }

        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Insufficient Crontab/Task permissions for all target scopes"
                .to_owned(),
        )
        .into());
    }

    permissions.retain(|perm| matches!(perm, Permission::Crontab(CrontabPermission::Write)));
    let has_crontab_write = check_token_limit(token_or_auth, scopes, permissions).await?;
    if !has_crontab_write {
        warn!(target: "crontab", "crontab write permission denied in global scope");
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Missing crontab write permission in global scope".to_owned(),
        )
        .into());
    }

    if let CronType::Server(ServerCronType::JsWorker(worker_name, _)) = cron_type {
        if worker_name.trim().is_empty() {
            return Err(NodegetError::InvalidInput(
                "Invalid crontab payload: js worker name cannot be empty".to_owned(),
            )
            .into());
        }

        let has_js_worker_run = check_token_limit(
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

pub async fn ensure_crontab_scope_permission(
    token_or_auth: &TokenOrAuth,
    cron_type: &CronType,
    permission: Permission,
    denied_message: &'static str,
) -> anyhow::Result<()> {
    trace!(target: "crontab", "checking crontab scope permission");
    let scopes = scopes_from_cron_type(cron_type);
    // Empty agent list: fall back to Global scope for permission check
    let scopes = if scopes.is_empty() {
        vec![Scope::Global]
    } else {
        scopes
    };
    let is_allowed = check_token_limit(token_or_auth, scopes, vec![permission]).await?;

    if is_allowed {
        Ok(())
    } else {
        warn!(target: "crontab", "crontab scope permission denied");
        Err(NodegetError::PermissionDenied(denied_message.to_owned()).into())
    }
}
