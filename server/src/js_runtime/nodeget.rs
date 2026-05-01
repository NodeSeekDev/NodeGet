use crate::js_runtime::js_error;
use crate::js_runtime::server_runtime::spawn_on_server_runtime;
use crate::rpc::get_modules;
use futures_util::future::join_all;
use rquickjs::Error;
use serde_json::value::RawValue;
use std::result::Result as StdResult;
use tracing::{debug, trace};

async fn raw_single_request(json: &str) -> StdResult<String, Error> {
    trace!(target: "js_runtime", "processing raw JSON-RPC request from JS");
    let rpc_module = get_modules();
    let json = json.to_owned();

    let response = spawn_on_server_runtime(async move {
        let (resp, _stream) = rpc_module
            .raw_json_request(&json, 16)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(resp.to_string())
    })
    .await
    .map_err(|e| js_error("jsonrpc_module", e))?;

    response.map_err(|e| js_error("jsonrpc_module", e))
}

/// # Errors
/// Returns an error if the JSON-RPC request fails.
pub async fn js_nodeget(json: String) -> StdResult<String, Error> {
    debug!(target: "js_runtime", "handling JS nodeget RPC call");
    let trimmed = json.trim();

    // Batch request: JSON array of JSON-RPC requests
    if trimmed.starts_with('[') {
        // Use RawValue to avoid parse→serialize→parse round-trip
        let items: Vec<Box<RawValue>> =
            serde_json::from_str(trimmed).map_err(|e| js_error("jsonrpc_parse", e.to_string()))?;

        let futs: Vec<_> = items
            .iter()
            .map(|item| {
                let req_str = item.get();
                async move { raw_single_request(req_str).await }
            })
            .collect();

        let results = join_all(futs).await;

        let mut responses = Vec::with_capacity(results.len());
        for result in results {
            responses.push(result?);
        }

        Ok(format!("[{}]", responses.join(",")))
    } else {
        // Single request
        raw_single_request(trimmed).await
    }
}
