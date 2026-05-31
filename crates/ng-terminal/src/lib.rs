//! ng-terminal: WebSocket terminal relay for NodeGet.
//!
//! ## `server` feature
//! - [`auth`] — terminal permission checking via [`TokenPermissionChecker`] trait
//! - [`check_agent`] — verify agent is connected and authorized for terminal
//! - [`router()`] — axum Router for WebSocket terminal endpoint
//! - [`TerminalState`] — shared state for terminal sessions
//! - [`TerminalSessionKey`] — session key (agent_uuid, terminal_id)
//! - [`SessionSlots`] — session channels between user and agent

#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
mod check_agent;
#[cfg(feature = "server")]
mod terminal;

#[cfg(feature = "server")]
pub use auth::{
    TokenPermissionChecker, check_terminal_connect_permission, get_token_checker, set_token_checker,
};
#[cfg(feature = "server")]
pub use check_agent::check_agent;
#[cfg(feature = "server")]
pub use terminal::{
    SessionSlots, TerminalParams, TerminalSessionKey, TerminalState, router, terminal_ws_handler,
};
