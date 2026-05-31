use crate::auth::get_token_checker;
use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;

/// Check if a token is a super token (used by list RPC).
pub async fn check_super_token(token: &str) -> anyhow::Result<bool> {
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
    get_token_checker()
        .check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")).into())
}
