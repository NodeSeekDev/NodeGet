use jsonrpsee::core::{JsonRawValue, SubscriptionResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::PendingSubscriptionSink;
use jsonrpsee::SubscriptionMessage;
use log::info;
use migration::async_trait::async_trait;
use nodeget_lib::task::TaskEvent;
use nodeget_lib::task::TaskEventType;
use nodeget_lib::utils::error_message::generate_error_message;
use nodeget_lib::utils::generate_random_string;
use serde_json::ser::Compound::RawValue;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

#[rpc(server, namespace = "task")]
pub trait Rpc {
    #[subscription(name = "register_task", item = TaskEvent, unsubscribe = "unregister_task")]
    async fn register_task(&self, uuid: Uuid) -> SubscriptionResult;

    #[method(name = "create_task")]
    async fn create_task(
        &self,
        token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> Value;
}

pub struct TaskRpcImpl {
    pub manager: TaskManager,
}

#[async_trait]
impl RpcServer for TaskRpcImpl {
    async fn create_task(
        &self,
        _token: String,
        target_uuid: Uuid,
        task_type: TaskEventType,
    ) -> Value {
        let task = TaskEvent {
            task_id: 0,
            task_token: generate_random_string(10),
            task_event_type: task_type,
        };

        let task_id = task.task_id;

        match self.manager.send_event(target_uuid, task).await {
            Ok(_) => {
                json!({ "task_id": task_id })
            }
            Err(e) => generate_error_message(e.0, e.1.as_str()),
        }
    }

    async fn register_task(
        &self,
        subscription_sink: PendingSubscriptionSink,
        uuid: Uuid,
    ) -> SubscriptionResult {
        let sink = subscription_sink.accept().await?;

        let (tx, mut rx) = mpsc::channel(32);

        let reg_id = Uuid::new_v4();

        self.manager.add_session(uuid, reg_id, tx).await;

        let manager_clone = self.manager.clone();
        let uuid_clone = uuid;
        let reg_id_clone = reg_id;

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(msg) => {
                        let sub_msg = SubscriptionMessage::from(
                            JsonRawValue::from_string(serde_json::to_string(&msg).unwrap())
                                .unwrap(),
                        );

                        if sink.send(sub_msg).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }

            manager_clone
                .remove_session(&uuid_clone, &reg_id_clone)
                .await;
            info!(
                "Client {} (RegID: {}) disconnected, logic handled.",
                uuid_clone, reg_id_clone
            );
        });

        Ok(())
    }
}

// Task 连接池
#[derive(Clone)]
pub struct TaskManager {
    peers: Arc<RwLock<HashMap<Uuid, (Uuid, mpsc::Sender<TaskEvent>)>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_session(&self, uuid: Uuid, reg_id: Uuid, tx: mpsc::Sender<TaskEvent>) {
        self.peers.write().await.insert(uuid, (reg_id, tx));
    }

    pub async fn remove_session(&self, uuid: &Uuid, reg_id: &Uuid) {
        let mut peers = self.peers.write().await;

        if let Some((current_reg_id, _)) = peers.get(uuid) {
            if current_reg_id == reg_id {
                peers.remove(uuid);
            }
        }
    }

    pub async fn send_event(&self, uuid: Uuid, event: TaskEvent) -> Result<(), (u32, String)> {
        let peers = self.peers.read().await;

        if let Some((_, tx)) = peers.get(&uuid) {
            tx.send(event)
                .await
                .map_err(|_| (104, "Error sending event".to_string()))
        } else {
            Err((103, "Uuid not found".to_string()))
        }
    }
}