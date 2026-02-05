// 任务创建和上传模块
mod create_upload_task;
// 任务查询模块
mod query;

use crate::rpc::RpcHelper;
use crate::token::get::check_token_limit;
use jsonrpsee::PendingSubscriptionSink;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::core::{JsonRawValue, RpcResult, SubscriptionResult};
use jsonrpsee::proc_macros::rpc;
use log::{error, info};
use migration::async_trait::async_trait;
use nodeget_lib::permission::data_structure::{Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::task::TaskEventType;
use nodeget_lib::task::query::TaskDataQuery;
use nodeget_lib::task::{TaskEvent, TaskEventResponse};
use nodeget_lib::utils::JsonError;
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

// 任务管理相关的 RPC 接口定义，包括任务注册、创建、结果上传和查询功能
#[rpc(server, namespace = "task")]
pub trait Rpc {
    // 任务订阅接口，允许客户端注册接收任务事件
    //
    // # 参数
    // * `token` - 认证令牌
    // * `uuid` - Agent 的 UUID
    //
    // # 返回值
    // 返回订阅结果
    #[subscription(name = "register_task", item = TaskEvent, unsubscribe = "unregister_task")]
    async fn register_task(&self, token: String, uuid: Uuid) -> SubscriptionResult;

    // 创建任务方法
    //
    // # 参数
    // * `token` - 认证令牌
    // * `target_uuid` - 目标 Agent 的 UUID
    // * `task_type` - 任务事件类型
    //
    // # 返回值
    // 返回创建任务的结果
    #[method(name = "create_task")]
    async fn create_task(
        &self,
        token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> Value;

    // 上传任务执行结果方法
    //
    // # 参数
    // * `token` - 认证令牌
    // * `task_response` - 任务事件响应
    //
    // # 返回值
    // 返回上传结果
    #[method(name = "upload_task_result")]
    async fn upload_task_result(&self, token: String, task_response: TaskEventResponse) -> Value;

    // 查询任务数据方法
    //
    // # 参数
    // * `token` - 认证令牌
    // * `task_data_query` - 任务数据查询条件
    //
    // # 返回值
    // 返回查询结果的原始 JSON 值
    #[method(name = "query")]
    async fn query(
        &self,
        token: String,
        task_data_query: TaskDataQuery,
    ) -> RpcResult<Box<RawValue>>;
}

// 任务管理 RPC 实现结构体
pub struct TaskRpcImpl {
    // 任务管理器实例
    pub manager: TaskManager,
}

// 为 TaskRpcImpl 实现 RPC 辅助功能
impl RpcHelper for TaskRpcImpl {}

#[async_trait]
impl RpcServer for TaskRpcImpl {
    // 创建任务实现
    //
    // # 参数
    // * `token` - 认证令牌
    // * `target_uuid` - 目标 Agent 的 UUID
    // * `task_type` - 任务事件类型
    //
    // # 返回值
    // 返回创建任务的结果
    async fn create_task(
        &self,
        token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> Value {
        create_upload_task::create_task(&self.manager, token, target_uuid, task_type).await
    }

    // 上传任务结果实现
    //
    // # 参数
    // * `token` - 认证令牌
    // * `task_response` - 任务事件响应
    //
    // # 返回值
    // 返回上传结果
    async fn upload_task_result(&self, token: String, task_response: TaskEventResponse) -> Value {
        create_upload_task::upload_task_result(token, task_response).await
    }

    // 查询任务数据实现
    //
    // # 参数
    // * `token` - 认证令牌
    // * `task_data_query` - 任务数据查询条件
    //
    // # 返回值
    // 返回查询结果的原始 JSON 值
    async fn query(
        &self,
        token: String,
        task_data_query: TaskDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query::query(token, task_data_query).await
    }

    // 任务注册实现，建立订阅连接
    //
    // # 参数
    // * `subscription_sink` - 订阅接收器
    // * `token` - 认证令牌
    // * `uuid` - Agent 的 UUID
    //
    // # 返回值
    // 返回订阅结果
    async fn register_task(
        &self,
        subscription_sink: PendingSubscriptionSink,
        token: String,
        uuid: Uuid,
    ) -> SubscriptionResult {
        let Ok(token_or_auth) = TokenOrAuth::from_full_token(&token) else {
            subscription_sink
                .reject(jsonrpsee::types::ErrorObject::borrowed(
                    101,
                    "Token Parse Error",
                    None,
                ))
                .await;
            return Ok(());
        };

        let is_allowed_result = check_token_limit(
            &token_or_auth,
            vec![Scope::AgentUuid(uuid)],
            vec![Permission::Task(Task::Listen)],
        )
        .await;

        match is_allowed_result {
            Ok(true) => {}
            Ok(false) => {
                subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::borrowed(
                        102,
                        "Permission Denied: Missing Task Listen permission for this Agent",
                        None,
                    ))
                    .await;
                return Ok(());
            }
            Err((code, msg)) => {
                let () = subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        code as i32,
                        msg.as_str(),
                        None::<JsonError>,
                    ))
                    .await;
                return Ok(());
            }
        }

        let sink = subscription_sink.accept().await?;
        let (tx, mut rx) = mpsc::channel(32);
        let reg_id = Uuid::new_v4();

        self.manager.add_session(uuid, reg_id, tx).await;

        let manager_clone = self.manager.clone();
        let uuid_clone = uuid;
        let reg_id_clone = reg_id;

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json_str = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to serialize task event: {e}");
                        break;
                    }
                };

                let Ok(raw_value) = JsonRawValue::from_string(json_str) else {
                    error!("Failed to create JsonRawValue");
                    break;
                };

                let sub_msg = SubscriptionMessage::from(raw_value);

                if sink.send(sub_msg).await.is_err() {
                    break;
                }
            }

            manager_clone
                .remove_session(&uuid_clone, &reg_id_clone)
                .await;
            info!("Client {uuid_clone} (RegID: {reg_id_clone}) disconnected, logic handled.");
        });

        Ok(())
    }
}

// 任务连接池类型别名，存储 Agent UUID 到会话 ID 和发送通道的映射
type Peers = Arc<RwLock<HashMap<Uuid, (Uuid, mpsc::Sender<TaskEvent>)>>>;
// 任务管理器，负责管理任务订阅和事件分发
#[derive(Clone)]
pub struct TaskManager {
    // 存储任务连接的并发安全映射
    peers: Peers,
}

impl TaskManager {
    // 创建新的任务管理器实例
    //
    // # 返回值
    // 返回初始化的任务管理器
    #[must_use]
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // 为指定 UUID 添加会话
    //
    // # 参数
    // * `uuid` - Agent 的 UUID
    // * `reg_id` - 注册 ID
    // * `tx` - 任务事件发送通道
    pub async fn add_session(&self, uuid: Uuid, reg_id: Uuid, tx: mpsc::Sender<TaskEvent>) {
        self.peers.write().await.insert(uuid, (reg_id, tx));
    }

    // 移除指定 UUID 的会话
    //
    // # 参数
    // * `uuid` - Agent 的 UUID
    // * `reg_id` - 注册 ID
    pub async fn remove_session(&self, uuid: &Uuid, reg_id: &Uuid) {
        let mut peers = self.peers.write().await;

        if let Some((current_reg_id, _)) = peers.get(uuid)
            && current_reg_id == reg_id
        {
            peers.remove(uuid);
        }
    }

    // 向指定 UUID 的 Agent 发送任务事件
    //
    // # 参数
    // * `uuid` - Agent 的 UUID
    // * `event` - 任务事件
    //
    // # 返回值
    // 成功返回空值，失败返回错误代码和消息
    pub async fn send_event(&self, uuid: Uuid, event: TaskEvent) -> Result<(), (u32, String)> {
        let peers = self.peers.read().await;

        if let Some((_, tx)) = peers.get(&uuid) {
            tx.send(event)
                .await
                .map_err(|_| (104, "Error sending event".to_string()))
        } else {
            Err((106, "Uuid not found".to_string()))
        }
    }
}
