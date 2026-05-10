// 监控数据报告模块
pub mod monitoring_data_report;
// 多服务器连接管理模块
pub mod multi_server;

use crate::config_access::get_agent_config;
use crate::rpc::multi_server::subscribe_to;
use log::{error, info, warn};
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::task::TaskEvent;
use nodeget_lib::utils::JsonError;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time;
use tokio_tungstenite::tungstenite::Message;

// `get_agent_config_safe` 的旧名，作为兼容别名保留，防止外部模块未同步更新引用。
// 新代码请直接用 `crate::config_access::get_agent_config`。
#[deprecated(note = "use crate::config_access::get_agent_config instead")]
#[allow(dead_code)]
pub fn get_agent_config_safe() -> anyhow::Result<AgentConfig> {
    get_agent_config().map_err(Into::into)
}

// JSON-RPC 2.0 请求结构体
#[derive(Serialize, Deserialize)]
struct JsonRpc {
    jsonrpc: String,                // JSON-RPC 版本号，固定为 "2.0"
    id: u64,                        // 请求ID，用于匹配响应
    method: String,                 // 要调用的方法名
    params: Vec<serde_json::Value>, // 方法参数
}

// 将方法和参数包装成 JSON-RPC 2.0 格式的字符串，使用 ID 1
//
// # 参数
// * `method` - 要调用的方法名
// * `params` - 方法参数向量
//
// # 返回值
// 返回 JSON-RPC 2.0 格式的字符串
pub fn wrap_json_into_rpc_with_id_1(method: &str, params: Vec<serde_json::Value>) -> String {
    let rpc = JsonRpc {
        jsonrpc: "2.0".to_owned(),
        id: 1,
        method: method.to_owned(),
        params,
    };

    // 这个序列化不应该失败，因为结构体只包含基本类型
    // 但如果失败，返回一个错误响应而不是panic
    serde_json::to_string(&rpc).unwrap_or_else(|e| {
        format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32603,"message":"Internal error: failed to serialize request: {e}"}}}}"#)
    })
}

// JSON-RPC 任务结构体，用于接收服务器下发的任务
#[derive(Serialize, Deserialize)]
pub struct JsonRpcTask {
    pub jsonrpc: String,           // JSON-RPC 版本号
    pub method: String,            // 方法名
    pub params: JsonRpcTaskResult, // 任务参数
}

// JSON-RPC 任务结果结构体
#[derive(Serialize, Deserialize)]
pub struct JsonRpcTaskResult {
    pub result: TaskEvent, // 任务事件
}

// JSON-RPC 错误消息结构体
#[derive(Serialize, Deserialize)]
pub struct JsonRpcErrorMessage {
    pub result: JsonError, // 错误信息
}

// 处理来自服务器的错误消息
//
// 该函数订阅各个服务器的错误消息通道，并打印接收到的错误信息。
//
// 每个 server 的订阅循环以及其派生的逐条处理任务都放入同一个
// [`JoinSet`]，并在函数 await 点上托管其所有权。当调用方（例如
// 配置热重载时）abort 了本函数的顶层 JoinHandle，`JoinSet` 会被
// drop 并自动 abort 所有子任务，避免新旧订阅并存。
pub async fn handle_error_message() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = match get_agent_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to get agent config: {e}");
            return;
        }
    };

    let mut tasks = JoinSet::new();

    for server in agent_config.server.unwrap_or_default() {
        tasks.spawn(async move {
            let mut rx = match subscribe_to(server.name.as_str()).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("[{}] Handle Error Message Error: {}", server.name, e);
                    return;
                }
            };

            let mut per_message_tasks = JoinSet::new();

            loop {
                while let Some(join_result) = per_message_tasks.try_join_next() {
                    if let Err(e) = join_result {
                        if !e.is_cancelled() {
                            warn!("[{}] Error handler task failed: {e}", server.name);
                        }
                    }
                }

                let message = match rx.recv().await {
                    Ok(msg) => msg,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            "[{}] Error handler lagged, dropped {n} messages",
                            server.name
                        );
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("[{}] Error handler channel closed", server.name);
                        break;
                    }
                };
                let server_name = server.name.clone();
                per_message_tasks.spawn(async move {
                    let rpc = match message {
                        Message::Text(text) => text.to_string(),
                        _ => {
                            return;
                        }
                    };

                    let Ok(json) = serde_json::from_str::<JsonRpcErrorMessage>(&rpc) else {
                        return;
                    };

                    warn!(
                        "[{}] Received Error Message: {}: {}",
                        server_name, json.result.error_id, json.result.error_message
                    );
                });
            }

            // drop per_message_tasks -> aborts any in-flight per-message processing
            drop(per_message_tasks);
        });
    }

    // Keep the JoinSet alive; if this future is aborted (e.g. on config
    // reload) the JoinSet is dropped and all per-server tasks are aborted
    // transitively, preventing the "old + new subscription coexist" leak
    // described in review_agent.md #9.
    while tasks.join_next().await.is_some() {}
}
