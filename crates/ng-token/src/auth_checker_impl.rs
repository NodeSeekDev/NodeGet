use ng_core::permission::data_structure::Token;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_infra::server::AuthChecker;

use crate::get::get_token;

/// Concrete `AuthChecker` that delegates to `ng_token::get::get_token`.
///
/// Registered via `ng_token::register_auth_checker()` during server startup.
///
/// # Warning
///
/// This implementation uses `tokio::task::block_in_place` + `Handle::block_on`
/// to bridge the sync `AuthChecker::check` trait method to the async `get_token`
/// function. It MUST only be called from within a tokio multi-thread runtime.
/// Calling from a non-runtime context or from within a `block_on` call will panic.
pub struct TokenAuthChecker;

impl AuthChecker for TokenAuthChecker {
    fn check(&self, raw_token: &str) -> anyhow::Result<Token> {
        let token_or_auth = TokenOrAuth::from_full_token(raw_token)
            .map_err(|e| ng_core::error::NodegetError::ParseError(e.to_string()))?;

        // Bridge async → sync: block_in_place allows the runtime to proceed
        // with other tasks while we block this thread on the async operation.
        // This is safe inside tokio::spawn or a multi-thread runtime handler.
        tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(get_token(&token_or_auth))
        })
    }
}
