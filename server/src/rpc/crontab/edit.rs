use crate::entity::crontab;
use crate::rpc::RpcHelper;
use crate::rpc::crontab::CrontabRpcImpl;
use crate::rpc::crontab::auth::{
    ensure_crontab_payload_write_permission, ensure_crontab_scope_permission, parse_cron_type,
};
use cron::Schedule;
use jsonrpsee::core::RpcResult;
use nodeget_lib::crontab::CronType;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Crontab as CrontabPermission, Permission};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use std::str::FromStr;

pub async fn edit(
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

        let db = CrontabRpcImpl::get_db()?;

        let model = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("Crontab not found: {name}")))?;

        let original_cron_type = parse_cron_type(&model.cron_type, &name)?;

        // 编辑已有 Crontab 前，必须覆盖其原有全部 Scope。
        ensure_crontab_scope_permission(
            &token_or_auth,
            &original_cron_type,
            Permission::Crontab(CrontabPermission::Write),
            "Permission Denied: Missing Crontab Write permission for all existing scopes",
        )
        .await?;

        // 新配置本身也必须满足完整 Scope + Task(Create) 写入权限。
        ensure_crontab_payload_write_permission(&token_or_auth, &cron_type).await?;

        let mut active_model: crontab::ActiveModel = model.into();
        active_model.cron_expression = Set(cron_expression);
        active_model.cron_type = CrontabRpcImpl::try_set_json(&cron_type)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let json_str = format!("{{\"id\":{},\"success\":true}}", updated.id);
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
