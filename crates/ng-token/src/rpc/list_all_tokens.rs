use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Token;
use ng_core::permission::token_auth::TokenOrAuth;
use serde::Serialize;
use serde_json::value::RawValue;
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::super_token::check_super_token;

#[derive(Serialize)]
struct ListAllTokensResponse {
    tokens: Vec<Token>,
}

pub async fn list_all_tokens(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "token", "processing list all tokens request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super_token {
            warn!(target: "token", "non-supertoken attempted to list all tokens");
            return Err(NodegetError::PermissionDenied(
                "Only SuperToken can list all tokens".to_owned(),
            )
            .into());
        }

        let cached_tokens = TokenCache::global().get_all();

        let tokens: Vec<Token> = cached_tokens
            .iter()
            .map(|entry| Token {
                version: entry.model.version,
                token_key: entry.model.token_key.clone(),
                timestamp_from: entry.model.time_stamp_from,
                timestamp_to: entry.model.time_stamp_to,
                token_limit: entry.parsed_limits.clone(),
                username: entry.model.username.clone(),
            })
            .collect();

        let response = ListAllTokensResponse { tokens };

        debug!(target: "token", token_count = response.tokens.len(), "list_all_tokens completed");
        let json_str = serde_json::to_string(&response).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize token list: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
