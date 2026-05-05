use crate::logging;
use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::debug;

pub async fn query_logs(token: String) -> RpcResult<Box<RawValue>> {
    debug!(target: "server", "querying in-memory logs");
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Super token required".to_owned(),
            )
            .into());
        }
        debug!(target: "server", "Super token verified for log query");

        let logs = logging::get_memory_logs();
        debug!(target: "server", log_count = logs.len(), "In-memory logs fetched");

        let json_str = serde_json::to_string(&logs)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

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
