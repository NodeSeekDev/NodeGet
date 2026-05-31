use crate::rpc::crontab::CrontabRpcImpl;
use ng_core::error::NodegetError;
use ng_core::utils::generate_random_string;
use ng_db::entity::{crontab_result, task};
use ng_db::get_db;
use ng_infra::server::RpcHelper;
use ng_js_runtime::RunType;
use ng_task::{TaskEvent, TaskEventType, TaskManager};
use sea_orm::{ActiveValue, EntityTrait, Set};
use tracing::{Instrument, debug, error, info, info_span, warn};
use uuid::Uuid;

// ── JsWorkerScheduler trait ──────────────────────────────────────────

/// Trait for enqueuing JS worker runs, injected by the server layer.
///
/// The ng-js-worker crate provides a concrete implementation that wraps
/// `enqueue_defined_js_worker_run`. This decouples ng-crontab from
/// ng-js-worker's internal module structure.
pub trait JsWorkerScheduler: Send + Sync + 'static {
    fn enqueue_run(
        &self,
        worker_name: String,
        run_type: RunType,
        params: serde_json::Value,
        env_override: Option<serde_json::Value>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send>>;
}

static JS_WORKER_SCHEDULER: std::sync::OnceLock<std::sync::Arc<dyn JsWorkerScheduler>> =
    std::sync::OnceLock::new();

/// Set the global JS worker scheduler for cron task execution.
pub fn set_js_worker_scheduler(scheduler: std::sync::Arc<dyn JsWorkerScheduler>) {
    let _ = JS_WORKER_SCHEDULER.set(scheduler);
}

/// Get the global JS worker scheduler.
pub fn js_worker_scheduler() -> Option<&'static std::sync::Arc<dyn JsWorkerScheduler>> {
    JS_WORKER_SCHEDULER.get()
}

// ── Agent task dispatch ──────────────────────────────────────────────

pub async fn crontab_task(
    cron_id: i64,
    cron_name: String,
    uuids: Vec<Uuid>,
    task_event_type: TaskEventType,
) {
    let span = info_span!(
        target: "crontab",
        "crontab::dispatch_task",
        cron_id,
        cron_name = %cron_name,
    );

    async {
        let db = match get_db() {
            Some(db) => db,
            None => {
                error!(
                    target: "crontab",
                    "failed to get DB connection for crontab task"
                );
                return;
            }
        };

        let agent_count = uuids.len();
        info!(
            target: "crontab",
            agent_count,
            task_type = ?task_event_type,
            "dispatching task to agents"
        );

        for uuid in uuids {
            let process_logic =
                async {
                    let token = generate_random_string(10);

                    let in_data = task::ActiveModel {
                        id: ActiveValue::default(),
                        uuid: Set(uuid),
                        token: Set(token.clone()),
                        cron_source: Set(Some(cron_name.clone())),
                        timestamp: Set(None),
                        success: Set(None),
                        error_message: Set(None),
                        task_event_type: <CrontabRpcImpl as RpcHelper>::try_set_json(
                            task_event_type.clone(),
                        )
                        .map_err(|e| NodegetError::SerializationError(format!("{e}")))?,
                        task_event_result: Set(None),
                    };

                    let result = task::Entity::insert(in_data).exec(db).await.map_err(|e| {
                        error!(
                            target: "crontab",
                            agent_uuid = %uuid,
                            error = %e,
                            "database insert error"
                        );
                        NodegetError::DatabaseError(format!("Database insert error: {e}"))
                    })?;

                    let task_id = result.last_insert_id;
                    debug!(
                        target: "crontab",
                        agent_uuid = %uuid,
                        task_id,
                        "task record inserted"
                    );

                    let task = TaskEvent {
                        task_id: task_id.cast_unsigned(),
                        task_token: token,
                        task_event_type: task_event_type.clone(),
                    };

                    let manager = TaskManager::global();

                    match manager.send_event(uuid, task).await {
                        Ok(()) => {
                            info!(
                                target: "crontab",
                                agent_uuid = %uuid,
                                task_id,
                                "task event sent to agent"
                            );
                            Ok(task_id)
                        }
                        Err(e) => {
                            let _ = task::Entity::delete_by_id(task_id).exec(db).await.map_err(
                                |del_err| {
                                    error!(
                                        target: "crontab",
                                        agent_uuid = %uuid,
                                        task_id,
                                        error = %del_err,
                                        "database delete error during rollback"
                                    );
                                    NodegetError::DatabaseError(format!(
                                        "Database delete error: {del_err}"
                                    ))
                                },
                            );
                            error!(
                                target: "crontab",
                                agent_uuid = %uuid,
                                task_id,
                                error = %e.1,
                                "failed to send task event to agent"
                            );
                            Err(NodegetError::AgentConnectionError(format!(
                                "Error sending task event: {}",
                                e.1
                            )))
                        }
                    }
                };

            // 执行逻辑并获取结果状态
            let (success, message, task_id) = match process_logic.await {
                Ok(new_id) => (
                    true,
                    format!("任务下发成功，Agent：[{uuid}]，relative_id：{new_id}"),
                    Some(new_id),
                ),
                Err(e) => {
                    warn!(
                        target: "crontab",
                        agent_uuid = %uuid,
                        error = %e,
                        "task dispatch failed"
                    );
                    (
                        false,
                        format!("任务下发失败，Agent：[{uuid}]，错误：{e}"),
                        None,
                    )
                }
            };

            let crontab_log = crontab_result::ActiveModel {
                id: ActiveValue::NotSet,
                cron_id: Set(cron_id),
                cron_name: Set(cron_name.clone()),
                relative_id: Set(task_id),
                run_time: Set(Some(chrono::Utc::now().timestamp_millis())),
                success: Set(Some(success)),
                message: Set(Some(message)),
            };

            if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
                error!(
                    target: "crontab",
                    agent_uuid = %uuid,
                    error = %e,
                    "failed to save crontab_result"
                );
            }
        }
    }
    .instrument(span)
    .await;
}
