use crate::CronType;
use crate::cache::CrontabCache;
use crate::rpc::crontab::CrontabRpcImpl;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::crontab;
use ng_infra::server::RpcHelper;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use std::str::FromStr;
use tracing::debug;

pub async fn create(
    token: String,
    name: String,
    cron_expression: String,
    cron_type: CronType,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab", name = %name, "processing crontab create request");
        // 1. 先验证 Token 格式（低成本操作）
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        // 2. 再检查权限（防止未授权访问）
        super::auth::ensure_crontab_payload_write_permission(&token_or_auth, &cron_type).await?;
        debug!(target: "crontab", name = %name, "Crontab create permission check passed");

        // 3. 最后验证 Cron 表达式（高成本操作，防止 DoS）
        if let Err(e) = cron::Schedule::from_str(&cron_expression) {
            return Err(NodegetError::ParseError(format!("Invalid cron expression: {e}")).into());
        }

        debug!(target: "crontab", name = %name, cron_expression = %cron_expression, "Cron expression validated");

        let db = CrontabRpcImpl::get_db()?;

        let existing_job = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("{e}")))?;

        if existing_job.is_some() {
            return Err(
                NodegetError::InvalidInput(format!("Crontab name already exists: {name}")).into(),
            );
        }
        debug!(target: "crontab", name = %name, "Crontab name available, inserting");

        let cron_type_json = CrontabRpcImpl::try_set_json(&cron_type)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

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
        let res_id = inserted.id;

        debug!(target: "crontab", id = res_id, name = %inserted.name, "Crontab created successfully");

        if let Err(e) = CrontabCache::reload().await {
            tracing::error!(target: "crontab", error = %e, "failed to reload crontab cache after create");
        }

        let json_str = format!("{{\"id\":{res_id}}}");
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
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
