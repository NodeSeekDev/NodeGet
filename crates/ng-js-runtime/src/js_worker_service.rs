//! `JsWorkerService` trait -- injection point for js-worker callbacks.
//!
//! The runtime pool and `inline_call/nodeget` modules need to call back into
//! the js-worker service (which lives in `ng-js-worker`). To break the
//! circular dependency, we define a trait here and inject the implementation
//! via `OnceLock` at startup, the same pattern used by `AuthChecker` in
//! `ng-infra`.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use serde_json::Value;

/// Trait for js-worker service callbacks.
///
/// Implemented by `ng-js-worker` (or the server crate) and injected once
/// during startup via [`set_js_worker_service`].
pub trait JsWorkerService: Send + Sync + 'static {
    /// Execute an inline call and record the result in the DB.
    fn run_inline_call_and_record_result(
        &self,
        js_script_name: String,
        params: Value,
        timeout_sec: Option<f64>,
        inline_caller: Option<String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send>>;

    /// Get a cloned RPC module for dispatching internal JSON-RPC requests.
    fn get_rpc_module(
        &self,
    ) -> Pin<Box<dyn Future<Output = Box<dyn RawJsonDispatcher + Send>> + Send>>;
}

/// Trait for raw JSON-RPC dispatch, abstracting over `jsonrpsee::RpcModule`.
///
/// This avoids depending on `jsonrpsee` in `ng-js-runtime`.
pub trait RawJsonDispatcher: Send + Sync {
    /// Dispatch a raw JSON-RPC request string, return the response string.
    fn raw_json_request(
        &self,
        json: &str,
        buf_size: usize,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<(String, ())>> + Send + '_>>;
}

static JS_WORKER_SERVICE: OnceLock<Box<dyn JsWorkerService>> = OnceLock::new();

/// Set the global `JsWorkerService` implementation.
///
/// Must be called once during server startup.
pub fn set_js_worker_service(service: Box<dyn JsWorkerService>) {
    let _ = JS_WORKER_SERVICE.set(service);
}

/// Get the global `JsWorkerService`.
///
/// Panics if not initialized -- call [`set_js_worker_service`] first.
pub fn get_js_worker_service() -> &'static dyn JsWorkerService {
    JS_WORKER_SERVICE
        .get()
        .expect("JsWorkerService not initialized -- call set_js_worker_service first")
        .as_ref()
}
