use crate::db_registry::DbRegistryManager;
use crate::token::get::check_token_limit;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::{Db as DbPermission, Permission, Scope};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn list(token: String) -> RpcResult<Box<RawValue>> {
    let (tk, un) = crate::rpc::token_identity(&token);

    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_allowed = check_token_limit(
            &token_or_auth,
            vec![Scope::Global],
            vec![Permission::Db(DbPermission::List)],
        )
        .await?;

        if !is_allowed {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Requires Db::List in Global scope".to_owned(),
            )
            .into());
        }

        let mgr = DbRegistryManager::global();
        let all = mgr.list_all().await?;

        debug!(target: "db", token_key = tk, username = un, count = all.len(), "database list");

        let resp = serde_json::json!({
            "success": true,
            "data": all,
        });

        let json_str = serde_json::to_string(&resp)?;
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
