use crate::DB;
use crate::entity::crontab;
use crate::token::get::get_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::crontab::{Cron, CronType};
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{
    Crontab as CrontabPermission, Permission, Scope, Token,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use sea_orm::EntityTrait;
use serde_json::value::RawValue;
use std::collections::HashSet;
use uuid::Uuid;

pub async fn get(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let token_info = get_token(&token_or_auth).await?;

        let now = get_local_timestamp_ms_i64()?;

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

        let has_crontab_read_permission = token_info.token_limit.iter().any(|limit| {
            limit
                .permissions
                .iter()
                .any(|perm| matches!(perm, Permission::Crontab(CrontabPermission::Read)))
        });

        if !has_crontab_read_permission {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient Crontab Read permission".to_owned(),
            )
            .into());
        }

        let crontabs = extract_allowed_uuids(&token_info).await?;
        let json_str = serde_json::to_string(&crontabs).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize crontabs: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

async fn get_crontabs_by_uuids(uuids: Vec<Uuid>) -> anyhow::Result<Vec<Cron>> {
    let db = DB
        .get()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

    let models = crontab::Entity::find()
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(format!("{e}")))?;

    let uuid_set: HashSet<Uuid> = uuids.into_iter().collect();

    let mut crons = Vec::new();
    for model in models {
        let cron_type = parse_cron_type_strict(&model)?;

        let should_include = match &cron_type {
            CronType::Agent(agent_uuids, _) => agent_uuids.iter().any(|uuid| uuid_set.contains(uuid)),
            CronType::Server(_) => false,
        };

        if should_include {
            crons.push(Cron {
                id: model.id,
                name: model.name,
                enable: model.enable,
                cron_expression: model.cron_expression,
                cron_type,
                last_run_time: model.last_run_time,
            });
        }
    }

    Ok(crons)
}

async fn get_all_crontabs() -> anyhow::Result<Vec<Cron>> {
    let db = DB
        .get()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

    let models = crontab::Entity::find()
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    let mut crons = Vec::with_capacity(models.len());
    for model in models {
        let cron_type = parse_cron_type_strict(&model)?;
        crons.push(Cron {
            id: model.id,
            name: model.name,
            enable: model.enable,
            cron_expression: model.cron_expression,
            cron_type,
            last_run_time: model.last_run_time,
        });
    }

    Ok(crons)
}

fn parse_cron_type_strict(model: &crontab::Model) -> anyhow::Result<CronType> {
    serde_json::from_value::<CronType>(model.cron_type.clone()).map_err(|e| {
        NodegetError::SerializationError(format!(
            "Failed to parse cron_type for crontab '{}' (id {}): {e}",
            model.name, model.id
        ))
        .into()
    })
}

async fn extract_allowed_uuids(token_info: &Token) -> anyhow::Result<Vec<Cron>> {
    let mut has_global = false;
    let mut allowed_uuids: Vec<Uuid> = Vec::new();

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
                    allowed_uuids.push(*uuid);
                }
                Scope::KvNamespace(_) => {
                    // KvNamespace scope 不适用于 crontab 权限检查，忽略
                }
            }
        }
    }

    if has_global {
        get_all_crontabs().await
    } else if !allowed_uuids.is_empty() {
        get_crontabs_by_uuids(allowed_uuids).await
    } else {
        Ok(Vec::new())
    }
}
