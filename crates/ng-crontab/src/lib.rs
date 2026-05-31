//! ng-crontab: Cron type definitions and server-side crontab management.
//!
//! ## Default features (types only)
//! - [`Cron`] — cron job definition
//! - [`CronType`] — agent or server cron type
//! - [`AgentCronType`] — agent-side cron type (task dispatch)
//! - [`ServerCronType`] — server-side cron type (JS worker)
//! - [`CrontabResult`] — cron execution result record
//! - [`query`] — `CrontabResultQueryCondition`, `CrontabResultDataQuery`
//!
//! ## `server` feature
//! - [`cache::CrontabCache`] — DB-backed in-memory crontab cache
//! - [`server_cron`] — per-minute cron scheduler loop
//! - [`task`] — cron task execution (creates tasks for agent crons)
//! - `crontab` RPC namespace — create, delete, edit, get, set_enable
//! - `crontab_result` RPC namespace — query, delete

mod cron_type;
mod result;

pub use cron_type::{AgentCronType, Cron, CronType, ServerCronType};
pub use result::CrontabResult;
pub mod query;

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod cache;
#[cfg(feature = "server")]
pub mod rpc;
#[cfg(feature = "server")]
pub mod server_cron;
#[cfg(feature = "server")]
pub mod task;

// ── Server-only re-exports ──────────────────────────────────────────

#[cfg(feature = "server")]
pub use cache::CrontabCache;

#[cfg(feature = "server")]
pub use server_cron::{delete_crontab_by_name, init_crontab_worker, set_crontab_enable_by_name};

#[cfg(feature = "server")]
/// Build and return a merged RPC module with both `crontab` and `crontab_result` namespaces.
///
/// The caller should merge this into the main RPC module during startup:
/// ```ignore
/// main_module.merge(ng_crontab::rpc_module()).unwrap();
/// ```
pub fn rpc_module() -> jsonrpsee::RpcModule<()> {
    let mut module = jsonrpsee::RpcModule::new(());
    module
        .merge(rpc::crontab::rpc_module())
        .expect("Failed to merge crontab RPC");
    module
        .merge(rpc::crontab_result::rpc_module())
        .expect("Failed to merge crontab_result RPC");
    module
}
