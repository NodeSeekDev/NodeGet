use crate::token::cache::TokenCache;
use crate::token::get::parse_token_limit_with_compat;
use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::Token;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde::Serialize;
use serde_json::value::RawValue;
use tracing::{debug, warn};

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

        let token_models = TokenCache::global().get_all().await;

        let tokens = token_models
            .into_iter()
            .map(|model| -> anyhow::Result<Token> {
                let token_limit =
                    parse_token_limit_with_compat(model.token_limit.clone()).map_err(|e| {
                        NodegetError::SerializationError(format!(
                            "Failed to parse token_limit for token '{}': {e}",
                            model.token_key
                        ))
                    })?;

                Ok(Token {
                    version: model.version,
                    token_key: model.token_key.clone(),
                    timestamp_from: model.time_stamp_from,
                    timestamp_to: model.time_stamp_to,
                    token_limit,
                    username: model.username.clone(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
