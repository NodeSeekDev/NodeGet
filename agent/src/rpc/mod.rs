// 监控数据报告模块
pub mod monitoring_data_report;
// 多服务器连接管理模块
pub mod multi_server;

use crate::AGENT_CONFIG;
use crate::rpc::multi_server::subscribe_to;
use log::{error, info, warn};
use nodeget_lib::config::agent::AgentConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::TaskEvent;
use nodeget_lib::utils::JsonError;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::tungstenite::Message;

// 安全地获取 Agent 配置
fn get_agent_config_safe() -> anyhow::Result<AgentConfig> {
    AGENT_CONFIG
        .get()
        .ok_or_else(|| NodegetError::Other("Agent config not initialized".to_owned()))?
        .read()
        .map(|guard| guard.clone())
        .map_err(|_| NodegetError::Other("AGENT_CONFIG lock poisoned".to_owned()).into())
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
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
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
// 该函数订阅各个服务器的错误消息通道，并打印接收到的错误信息
pub async fn handle_error_message() {
    time::sleep(Duration::from_secs(1)).await;

    let agent_config = match get_agent_config_safe() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to get agent config: {e}");
            return;
        }
    };

    for server in agent_config.server.unwrap_or(vec![]) {
        tokio::spawn(async move {
            let mut rx = match subscribe_to(server.name.as_str()).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("[{}] Handle Error Message Error: {}", server.name, e);
                    return;
                }
            };

            loop {
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
                {
                    let message = message;
                    let server_name = server.name.clone();
                    tokio::spawn(async move {
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
            }
        });
    }
}
