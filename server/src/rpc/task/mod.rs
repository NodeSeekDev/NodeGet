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
use nodeget_lib::task::query::TaskDataQuery;
use nodeget_lib::task::{TaskEvent, TaskEventResponse, TaskEventType};
use nodeget_lib::utils::JsonError;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

mod create_task;
mod query;
mod upload_task_result;

#[rpc(server, namespace = "task")]
pub trait Rpc {
    #[subscription(name = "register_task", item = TaskEvent, unsubscribe = "unregister_task")]
    async fn register_task(&self, token: String, uuid: Uuid) -> SubscriptionResult;

    #[method(name = "create_task")]
    async fn create_task(
        &self,
        token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "upload_task_result")]
    async fn upload_task_result(
        &self,
        token: String,
        task_response: TaskEventResponse,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "query")]
    async fn query(
        &self,
        token: String,
        task_data_query: TaskDataQuery,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct TaskRpcImpl {
    pub manager: Arc<TaskManager>,
}

impl RpcHelper for TaskRpcImpl {}

#[async_trait]
impl RpcServer for TaskRpcImpl {
    async fn create_task(
        &self,
        token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> RpcResult<Box<RawValue>> {
        create_task::create_task(&self.manager, token, target_uuid, task_type).await
    }

    async fn upload_task_result(
        &self,
        token: String,
        task_response: TaskEventResponse,
    ) -> RpcResult<Box<RawValue>> {
        upload_task_result::upload_task_result(token, task_response).await
    }

    async fn query(
        &self,
        token: String,
        task_data_query: TaskDataQuery,
    ) -> RpcResult<Box<RawValue>> {
        query::query(token, task_data_query).await
    }

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
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                let () = subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        nodeget_err.error_code() as i32,
                        nodeget_err.to_string(),
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

type Peers = Arc<RwLock<HashMap<Uuid, (Uuid, mpsc::Sender<TaskEvent>)>>>;

static GLOBAL_TASK_MANAGER: OnceLock<Arc<TaskManager>> = OnceLock::new();

#[derive(Clone)]
pub struct TaskManager {
    peers: Peers,
}

impl TaskManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn global() -> &'static Arc<Self> {
        GLOBAL_TASK_MANAGER.get_or_init(|| Arc::new(Self::new()))
    }

    pub async fn add_session(&self, uuid: Uuid, reg_id: Uuid, tx: mpsc::Sender<TaskEvent>) {
        self.peers.write().await.insert(uuid, (reg_id, tx));
    }

    pub async fn remove_session(&self, uuid: &Uuid, reg_id: &Uuid) {
        let mut peers = self.peers.write().await;

        if let Some((current_reg_id, _)) = peers.get(uuid)
            && current_reg_id == reg_id
        {
            peers.remove(uuid);
        }
    }

    pub async fn send_event(&self, uuid: Uuid, event: TaskEvent) -> Result<(), (i32, String)> {
        let peers = self.peers.read().await;

        if let Some((_, tx)) = peers.get(&uuid) {
            tx.send(event)
                .await
                .map_err(|e| (103, format!("Failed to send task event: {e}")))?;
            Ok(())
        } else {
            Err((104, format!("Agent {uuid} is not connected")))
        }
    }
}
