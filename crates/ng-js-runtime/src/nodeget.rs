//! `nodeget()` API —— JS 上下文内部发起 JSON-RPC 调用。
//!
//! JS 端通过 `globalThis.__nodeget_rpc_raw(json)` 触发此模块，
//! 将请求转发到服务器 `RpcModule` 进行分发。支持单条和批量请求。

use crate::js_worker_service::get_js_worker_service;
use crate::server_runtime::js_error;
use crate::spawn_on_server_runtime::spawn_on_server_runtime;
use rquickjs::Error;
use serde_json::value::RawValue;
use std::result::Result as StdResult;
use tracing::{debug, trace};

/// RPC 响应缓冲区大小。
/// 足以容纳绝大多数 JSON-RPC 响应，避免因缓冲区过小导致截断重试。
const RPC_BUF_SIZE: usize = 4096;

/// 从 JS 上下文发起 `nodeget()` RPC 调用，返回响应 JSON 字符串。
///
/// - `json` —— JSON-RPC 请求字符串（单条或批量数组）
///
/// 内部步骤：
/// 1. 判断是否为批量请求（以 `[` 开头）
/// 2. 批量请求：单次获取 RPC module 后分发所有子请求，避免每条子请求独立 spawn
/// 3. 单条请求：直接通过 `spawn_on_server_runtime` 执行
///
/// # Errors
/// 若 JSON-RPC 请求执行失败，返回 `rquickjs::Error`。
pub async fn js_nodeget(json: String) -> StdResult<String, Error> {
    debug!(target: "js_runtime", "handling JS nodeget RPC call");
    let trimmed = json.trim();

    // 批量请求：JSON 数组形式，单次获取 RPC module 后依次分发
    if trimmed.starts_with('[') {
        // 使用 RawValue 避免 parse→serialize→parse 的往返开销
        let items: Vec<Box<RawValue>> =
            serde_json::from_str(trimmed).map_err(|e| js_error("jsonrpc_parse", e.to_string()))?;

        let items_len = items.len();
        let results = spawn_on_server_runtime(async move {
            let service = get_js_worker_service()
                .ok_or_else(|| "JsWorkerService not initialized".to_string())?;
            let rpc_module = service.get_rpc_module().await;
            let mut results = Vec::with_capacity(items_len);
            for item in &items {
                let req_str = item.get();
                let (resp, _stream) = rpc_module
                    .raw_json_request(req_str, RPC_BUF_SIZE)
                    .await
                    .map_err(|e| e.to_string())?;
                results.push(Ok::<_, String>(resp));
            }
            Ok::<_, String>(results)
        })
        .await
        .map_err(|e| js_error("jsonrpc_module", e))?
        .map_err(|e| js_error("jsonrpc_module", e))?;

        let mut responses = Vec::with_capacity(results.len());
        for result in results {
            responses.push(result.map_err(|e| js_error("jsonrpc_module", e))?);
        }

        Ok(format!("[{}]", responses.join(",")))
    } else {
        // 单条请求
        trace!(target: "js_runtime", "processing raw JSON-RPC request from JS");
        // 若 trim 未改变内容，直接复用原始 String，避免分配
        let json = if trimmed.len() == json.len() { json } else { trimmed.to_owned() };

        let response = spawn_on_server_runtime(async move {
            let service = get_js_worker_service()
                .ok_or_else(|| "JsWorkerService not initialized".to_string())?;
            let rpc_module = service.get_rpc_module().await;
            let (resp, _stream) = rpc_module
                .raw_json_request(&json, RPC_BUF_SIZE)
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(resp)
        })
        .await
        .map_err(|e| js_error("jsonrpc_module", e))?
        .map_err(|e| js_error("jsonrpc_module", e))?;

        Ok(response)
    }
}
