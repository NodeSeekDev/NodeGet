pub mod monitoring_data_report;
pub mod multi_server;

use nodeget_lib::task::TaskEvent;
use serde::{Deserialize, Serialize};
use nodeget_lib::utils::JsonError;

#[derive(Serialize, Deserialize)]
struct JsonRpc {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Vec<serde_json::Value>,
}

pub fn wrap_json_into_rpc_with_id_1(method: &str, params: Vec<serde_json::Value>) -> String {
    let rpc = JsonRpc {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params,
    };

    serde_json::to_string(&rpc).unwrap()
}

#[derive(Serialize, Deserialize)]
pub struct JsonRpcTask {
    pub jsonrpc: String,
    pub method: String,
    pub params: JsonRpcTaskResult,
}

#[derive(Serialize, Deserialize)]
pub struct JsonRpcTaskResult {
    pub result: TaskEvent,
}

#[derive(Serialize, Deserialize)]
pub struct JsonRpcErrorMessage {
    pub result: JsonError,
}


