use crate::rpc::crontab::CrontabRpcImpl;
use crate::server_cron::delete_crontab_by_name;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::data_structure::{Crontab as CrontabPermission, Permission};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::crontab;
use ng_infra::server::RpcHelper;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab", name = %name, "processing crontab delete request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let db = CrontabRpcImpl::get_db()?;
        let model = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let Some(model) = model else {
            return Err(NodegetError::NotFound(format!("Crontab not found: {name}")).into());
        };

        debug!(target: "crontab", id = model.id, name = %name, "Crontab found for deletion");

        let cron_type = super::auth::parse_cron_type(&model.cron_type, &name)?;
        super::auth::ensure_crontab_scope_permission(
            &token_or_auth,
            &cron_type,
            Permission::Crontab(CrontabPermission::Delete),
            "Permission Denied: Missing Crontab Delete permission for all target scopes",
        )
        .await?;

        debug!(target: "crontab", name = %name, "Crontab delete permission check passed");

        let deleted = delete_crontab_by_name(name.clone())
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to delete crontab: {e}")))?;

        if !deleted {
            return Err(NodegetError::NotFound(format!("Crontab not found: {name}")).into());
        }

        debug!(target: "crontab", name = %name, "Crontab deleted successfully");

        let json_str = format!("{{\"success\":{deleted}}}");
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
