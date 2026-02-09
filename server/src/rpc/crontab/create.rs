use crate::entity::crontab;
use crate::rpc::RpcHelper;
use crate::rpc::crontab::CrontabRpcImpl;
use crate::token::get::check_token_limit;
use cron::Schedule;
use jsonrpsee::core::RpcResult;
use nodeget_lib::crontab::{AgentCronType, CronType};
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{
    Crontab as CrontabPermission, Permission, Scope, Task,
};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use std::str::FromStr;

pub async fn create(
    token: String,
    name: String,
    cron_expression: String,
    cron_type: CronType,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        if let Err(e) = Schedule::from_str(&cron_expression) {
            return Err(NodegetError::ParseError(format!("Invalid cron expression: {e}")).into());
        }

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let mut scopes = Vec::new();
        let mut permissions = Vec::new();

        permissions.push(Permission::Crontab(CrontabPermission::Write));

        match &cron_type {
            CronType::Agent(uuids, agent_cron_type) => {
                if uuids.is_empty() {
                    return Err(NodegetError::ParseError("Agent list cannot be empty".to_string()).into());
                }
                for uuid in uuids {
                    scopes.push(Scope::AgentUuid(*uuid));
                }

                match agent_cron_type {
                    AgentCronType::Task(task_event_type) => {
                        permissions.push(Permission::Task(Task::Create(
                            task_event_type.task_name().to_string(),
                        )));
                    }
                }
            }
            CronType::Server(_) => {
                scopes.push(Scope::Global);
            }
        }

        let is_allowed = check_token_limit(&token_or_auth, scopes, permissions).await?;
        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient Crontab or Task permissions".to_string(),
            )
            .into());
        }

        let db = CrontabRpcImpl::get_db()?;

        let existing_job = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("{e}")))?;

        let cron_type_json = CrontabRpcImpl::try_set_json(&cron_type)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

        let res_id = if let Some(model) = existing_job {
            let mut active_model: crontab::ActiveModel = model.into();
            active_model.cron_expression = Set(cron_expression);
            active_model.cron_type = cron_type_json;
            active_model.enable = Set(true);

            let updated = active_model
                .update(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;
            updated.id
        } else {
            let new_model = crontab::ActiveModel {
                id: ActiveValue::NotSet,
                name: Set(name),
                cron_expression: Set(cron_expression),
                cron_type: cron_type_json,
                enable: Set(true),
                last_run_time: Set(None),
            };

            let inserted = new_model
                .insert(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;
            inserted.id
        };

        let json_str = format!("{{\"id\":{}}}", res_id);
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
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
