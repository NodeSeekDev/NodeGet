use crate::token::generate_token::generate_and_store_token;
use jsonrpsee::core::RpcResult;
use log::debug;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::create::TokenCreationRequest;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;

pub async fn create(father_token: String, token_creation: TokenCreationRequest) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let father_token_or_auth = TokenOrAuth::from_full_token(&father_token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        debug!("Token RPC: Processing token creation request");

        let (key, secret) = generate_and_store_token(
            &father_token_or_auth,
            token_creation.timestamp_from,
            token_creation.timestamp_to,
            token_creation.token_limit,
            token_creation.username,
            token_creation.password,
        )
        .await?;

        let json_str = format!(
            "{{\"key\":\"{}\",\"secret\":\"{}\"}}",
            key, secret
        );

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
