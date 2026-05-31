#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]

//! `ng-js-runtime` -- `QuickJS` runtime pool and bytecode compilation.
//!
//! ## Default feature (types only)
//! - [`JsCodeInput`] -- JS code input struct
//! - [`RunType`] -- enum for different run modes
//! - [`CompileMode`] -- bytecode vs source compilation mode
//! - [`RuntimePoolInfo`], [`RuntimePoolWorkerInfo`] -- runtime pool status types
//!
//! ## `server` feature
//! - [`compile_js_module_to_bytecode()`] -- compile JS to bytecode
//! - [`runtime_pool`] module -- OS thread pool of `QuickJS` instances
//! - [`server_runtime`] module -- server-level runtime operations (`spawn_on_server_runtime`)
//! - [`nodeget`] module -- `nodeget()` API injection into JS context
//! - [`inline_call`] module -- inline JS execution
//! - [`js_worker_service`] module -- `JsWorkerService` trait for dependency injection
//! - [`RuntimeLimits`] -- runtime resource limits
//! - [`js_runner`] / [`js_runner_source_mode`] -- one-shot JS execution

mod types;

pub use types::*;

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod inline_call;
#[cfg(feature = "server")]
pub mod js_worker_service;
#[cfg(feature = "server")]
pub mod nodeget;
#[cfg(feature = "server")]
pub mod runtime_pool;
#[cfg(feature = "server")]
pub mod server_runtime;
#[cfg(feature = "server")]
mod spawn_on_server_runtime;

// ── Server-only re-exports ──────────────────────────────────────────

#[cfg(feature = "server")]
pub use server_runtime::{
    RuntimeLimits, compile_js_module_to_bytecode, format_js_error, js_error, js_runner,
    js_runner_source_mode,
};
#[cfg(feature = "server")]
pub use spawn_on_server_runtime::{init as init_server_runtime, spawn_on_server_runtime};
