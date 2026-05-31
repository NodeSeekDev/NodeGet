use crate::CronType;
use crate::cache::CrontabCache;
use crate::rpc::crontab::CrontabRpcImpl;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::data_structure::{Crontab as CrontabPermission, Permission};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::crontab;
use ng_infra::server::RpcHelper;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::value::RawValue;
use std::str::FromStr;
use tracing::debug;

pub async fn edit(
    token: String,
    name: String,
    cron_expression: String,
    cron_type: CronType,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab", name = %name, "processing crontab edit request");
        // 1. 先验证 Token 格式（低成本操作）
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let db = CrontabRpcImpl::get_db()?;

        // 查询原有 cron 的类型，用于覆盖旧 Scope 的权限检查
        let original_model = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("{e}")))?
            .ok_or_else(|| NodegetError::NotFound(format!("Crontab not found: {name}")))?;

        let original_cron_type = super::auth::parse_cron_type(&original_model.cron_type, &name)?;

        // 2. 检查权限（防止未授权访问）
        // 编辑已有 Crontab 前，必须覆盖其原有全部 Scope
        super::auth::ensure_crontab_scope_permission(
            &token_or_auth,
            &original_cron_type,
            Permission::Crontab(CrontabPermission::Write),
            "Permission Denied: Missing Crontab Write permission for all existing scopes",
        )
        .await?;

        // 新配置本身也必须满足完整 Scope + Task(Create) 写入权限
        super::auth::ensure_crontab_payload_write_permission(&token_or_auth, &cron_type).await?;
        debug!(target: "crontab", name = %name, "Crontab edit permission checks passed");

        // 3. 最后验证 Cron 表达式（高成本操作，防止 DoS）
        if let Err(e) = cron::Schedule::from_str(&cron_expression) {
            return Err(NodegetError::ParseError(format!("Invalid cron expression: {e}")).into());
        }

        debug!(target: "crontab", name = %name, cron_expression = %cron_expression, "Cron expression validated");
        debug!(target: "crontab", id = original_model.id, name = %name, "Crontab found for editing");

        let mut active_model: crontab::ActiveModel = original_model.into();
        active_model.cron_expression = Set(cron_expression);
        active_model.cron_type = CrontabRpcImpl::try_set_json(&cron_type)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        debug!(target: "crontab", id = updated.id, name = %name, "Crontab edited successfully");

        if let Err(e) = CrontabCache::reload().await {
            tracing::error!(target: "crontab", error = %e, "failed to reload crontab cache after edit");
        }

        let json_str = format!("{{\"id\":{},\"success\":true}}", updated.id);
        RawValue::from_string(json_str)
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
