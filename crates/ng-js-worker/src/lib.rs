//! ng-js-worker: JS worker record management and RPC namespaces.
//!
//! ## Default features (types only)
//! - No significant standalone types (worker types are DB entities in `ng-db`)
//!
//! ## `server` feature
//! - `service` module — core JS worker service (enqueue runs, run inline calls, record results)
//! - `js_worker` RPC namespace — create, read, update, delete, list, run, get_rt_pool, service, route_name
//! - `js_result` RPC namespace — query, delete
//! - `auth` module — permission checking (via `TokenPermissionChecker` trait injection)
//! - `rpc_module()` — build and return merged RPC module for both namespaces

#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
pub mod js_result;
#[cfg(feature = "server")]
pub mod js_worker;
#[cfg(feature = "server")]
pub mod service;

#[cfg(feature = "server")]
pub use auth::{TokenPermissionChecker, get_token_checker, set_token_checker};

#[cfg(feature = "server")]
pub use service::{
    enqueue_defined_js_worker_run, enqueue_source_js_worker_run, run_inline_call_and_record_result,
};

#[cfg(feature = "server")]
/// Build and return the merged RPC module for both `js-worker` and `js-result` namespaces.
///
/// Call this during server startup to register both namespaces at once.
pub fn rpc_module() -> jsonrpsee::RpcModule<js_worker::JsWorkerRpcImpl> {
    let mut module = js_worker::rpc_module();
    let result_module = js_result::rpc_module();
    module
        .merge(result_module)
        .expect("failed to merge js_result RPC module");
    module
}
