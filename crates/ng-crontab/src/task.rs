//! 定时任务执行模块：定义 JsWorkerScheduler trait 注入及 Agent 任务下发逻辑。
//!
//! `JsWorkerScheduler` 由 Server 二进制在启动时通过 `set_js_worker_scheduler` 注入，
//! 解耦 ng-crontab 与 ng-js-worker 的内部模块结构。
//! Agent 类型定时任务通过 `crontab_task` 函数下发：批量构建 Task 记录，
//! 并发发送 TaskEvent，失败时批量回滚，最终批量写入 CrontabResult。

use crate::rpc::crontab::CrontabRpcImpl;
use ng_core::error::NodegetError;
use ng_core::utils::generate_random_string;
use ng_db::entity::{crontab_result, task};
use ng_db::get_db;
use ng_infra::server::RpcHelper;
use ng_js_runtime::RunType;
use ng_task::{TaskEvent, TaskEventType, TaskManager};
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use tokio::task::JoinSet;
use tracing::{Instrument, error, info, info_span, warn};
use uuid::Uuid;

// ── JsWorkerScheduler trait 注入 ─────────────────────────────────────

/// JS Worker 调度器 trait，由 Server 层注入具体实现。
///
/// ng-js-worker crate 提供具体实现，包装 `enqueue_defined_js_worker_run`，
/// 解耦 ng-crontab 与 ng-js-worker 的内部模块结构。
pub trait JsWorkerScheduler: Send + Sync + 'static {
    /// 将 JS Worker 运行请求加入调度队列。
    ///
    /// - `worker_name` - JS Worker 脚本名称
    /// - `run_type` - 运行类型（Cron / Manual 等）
    /// - `params` - 传入参数 JSON
    /// - `env_override` - 环境变量覆盖（可选）
    /// - 返回关联的 relative_id
    fn enqueue_run(
        &self,
        worker_name: String,
        run_type: RunType,
        params: serde_json::Value,
        env_override: Option<serde_json::Value>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send>>;
}

/// 全局 JsWorkerScheduler 单例，启动时由 Server 二进制通过 `set_js_worker_scheduler` 注入。
static JS_WORKER_SCHEDULER: std::sync::OnceLock<std::sync::Arc<dyn JsWorkerScheduler>> =
    std::sync::OnceLock::new();

/// 设置全局 JS Worker 调度器（启动时调用一次）。
pub fn set_js_worker_scheduler(scheduler: std::sync::Arc<dyn JsWorkerScheduler>) {
    let _ = JS_WORKER_SCHEDULER.set(scheduler);
}

/// 获取全局 JS Worker 调度器。
pub fn js_worker_scheduler() -> Option<&'static std::sync::Arc<dyn JsWorkerScheduler>> {
    JS_WORKER_SCHEDULER.get()
}

// ── Agent 任务下发 ────────────────────────────────────────────────

/// 向指定 Agent UUID 列表批量下发定时任务。
///
/// 1. 一次性序列化 `task_event_type`，批量构建所有 task ActiveModel
/// 2. 单次 `insert_many` 写入 task 记录，从 `last_insert_id` 推算连续 ID
/// 3. 并发发送 TaskEvent 到各 Agent（JoinSet 替代逐个 await）
/// 4. 批量回滚发送失败的 task 记录
/// 5. 单次 `insert_many` 写入 crontab_result
///
/// - `cron_id` - 定时任务 ID
/// - `cron_name` - 定时任务名称
/// - `uuids` - 目标 Agent UUID 列表
/// - `task_event_type` - 任务事件类型
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
        if agent_count == 0 {
            return;
        }
        info!(
            target: "crontab",
            agent_count,
            task_type = ?task_event_type,
            "dispatching task to agents"
        );

        // 序列化一次，所有 task 记录共享
        let task_event_type_value =
            <CrontabRpcImpl as RpcHelper>::try_set_json(task_event_type.clone())
                .map_err(|e| NodegetError::SerializationError(format!("{e}")));

        let task_event_type_value = match task_event_type_value {
            Ok(v) => v,
            Err(e) => {
                error!(target: "crontab", error = %e, "failed to serialize task_event_type");
                return;
            }
        };

        // 批量构建 task ActiveModel
        let mut tokens: Vec<String> = Vec::with_capacity(agent_count);
        let task_models: Vec<task::ActiveModel> = uuids
            .iter()
            .map(|uuid| {
                let token = generate_random_string(10);
                tokens.push(token.clone());
                task::ActiveModel {
                    id: ActiveValue::default(),
                    uuid: Set(*uuid),
                    token: Set(token),
                    cron_source: Set(Some(cron_name.clone())),
                    timestamp: Set(None),
                    success: Set(None),
                    error_message: Set(None),
                    task_event_type: task_event_type_value.clone(),
                    task_event_result: Set(None),
                }
            })
            .collect();

        // 批量 INSERT（单次 DB 往返），auto-increment 保证 ID 连续
        let insert_result = match task::Entity::insert_many(task_models).exec(db).await {
            Ok(r) => r,
            Err(e) => {
                error!(target: "crontab", error = %e, "batch task insert error");
                return;
            }
        };

        // 从 last_insert_id 推算连续 task_id，无需额外 SELECT
        let last_id = match insert_result.last_insert_id {
            Some(id) => id,
            None => {
                error!(target: "crontab", "batch insert returned no last_insert_id");
                return;
            }
        };
        let base_id = last_id - (agent_count as i64 - 1);

        // 并发发送任务事件
        let manager = TaskManager::global();
        let mut send_set = JoinSet::new();

        for (i, uuid) in uuids.iter().enumerate() {
            let task_id = base_id + i as i64;
            let task = TaskEvent {
                task_id: task_id.cast_unsigned(),
                task_token: tokens[i].clone(),
                task_event_type: task_event_type.clone(),
            };
            let uuid = *uuid;
            send_set.spawn(async move {
                (uuid, task_id, manager.send_event(uuid, task).await)
            });
        }

        // 收集发送结果
        let mut crontab_results: Vec<crontab_result::ActiveModel> = Vec::with_capacity(agent_count);
        let mut failed_task_ids: Vec<i64> = Vec::new();

        while let Some(res) = send_set.join_next().await {
            let (uuid, task_id, send_result) = match res {
                Ok(r) => r,
                Err(e) => {
                    error!(target: "crontab", error = %e, "send task panicked");
                    continue;
                }
            };

            match send_result {
                Ok(()) => {
                    info!(target: "crontab", agent_uuid = %uuid, task_id, "task event sent to agent");
                    crontab_results.push(crontab_result::ActiveModel {
                        id: ActiveValue::NotSet,
                        cron_id: Set(cron_id),
                        cron_name: Set(cron_name.clone()),
                        relative_id: Set(Some(task_id)),
                        run_time: Set(Some(chrono::Utc::now().timestamp_millis())),
                        success: Set(Some(true)),
                        message: Set(Some(format!(
                            "任务下发成功，Agent：[{uuid}]，relative_id：{task_id}"
                        ))),
                    });
                }
                Err(e) => {
                    warn!(
                        target: "crontab",
                        agent_uuid = %uuid,
                        task_id,
                        error = %e.1,
                        "failed to send task event to agent"
                    );
                    failed_task_ids.push(task_id);
                    crontab_results.push(crontab_result::ActiveModel {
                        id: ActiveValue::NotSet,
                        cron_id: Set(cron_id),
                        cron_name: Set(cron_name.clone()),
                        relative_id: Set(None),
                        run_time: Set(Some(chrono::Utc::now().timestamp_millis())),
                        success: Set(Some(false)),
                        message: Set(Some(format!(
                            "任务下发失败，Agent：[{uuid}]，错误：{}",
                            e.1
                        ))),
                    });
                }
            }
        }

        // 批量回滚发送失败的 task 记录
        if !failed_task_ids.is_empty()
            && let Err(e) = task::Entity::delete_many()
            .filter(task::Column::Id.is_in(failed_task_ids))
            .exec(db)
            .await
        {
            error!(target: "crontab", error = %e, "failed to batch delete failed task records");
        }

        // 批量写入 crontab_result（单次 DB 往返）
        if !crontab_results.is_empty()
            && let Err(e) = crontab_result::Entity::insert_many(crontab_results)
            .exec(db)
            .await
        {
            error!(target: "crontab", error = %e, "failed to batch save crontab_results");
        }
    }
        .instrument(span)
        .await;
}
