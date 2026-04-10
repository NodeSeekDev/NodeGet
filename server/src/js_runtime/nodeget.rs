use crate::js_runtime::js_error;
use crate::rpc::get_modules;
use futures::future::join_all;
use rquickjs::Error;
use std::result::Result as StdResult;

async fn raw_single_request(json: &str) -> StdResult<String, Error> {
    let rpc_module = get_modules();

    let (resp, _stream) = match rpc_module.raw_json_request(json, 16).await {
        Ok(resp) => resp,
        Err(e) => {
            return Err(js_error("jsonrpc_module", e.to_string()));
        }
    };

    Ok(resp.to_string())
}

pub async fn js_nodeget(json: String) -> StdResult<String, Error> {
    let trimmed = json.trim();

    // Batch request: JSON array of JSON-RPC requests
    if trimmed.starts_with('[') {
        let items: Vec<serde_json::Value> = serde_json::from_str(trimmed)
            .map_err(|e| js_error("jsonrpc_parse", e.to_string()))?;

        let futs: Vec<_> = items
            .into_iter()
            .map(|item| {
                let req_str = item.to_string();
                async move { raw_single_request(&req_str).await }
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
