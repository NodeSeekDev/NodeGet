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
/// 2. 单次 `insert_many` + `exec_with_returning` 写入 task 记录，用 RETURNING 取回每行真实 id
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

        // 批量构建 task ActiveModel（每个 uuid 一条，token 各自随机）
        let task_models: Vec<task::ActiveModel> = uuids
            .iter()
            .map(|uuid| {
                let token = generate_random_string(10);
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

        // 批量 INSERT 并用 RETURNING 取回每行的真实 Model。
        // ⚠️ 不能用 `last_insert_id` 反推连续区间：PostgreSQL 的 IDENTITY 序列 nextval 按行原子
        // 分配，并发 insert_many（多个 cron 同 tick、cron 与 task.create 并发）会让本批拿到的 id
        // 交错（如本批得 100/102/104），`base_id = last_id - (n-1)` 会把属于别的批次的 id 当作
        // 本批任务下发，导致 task_id+uuid+token 错配（详见 upload_task_result 三元组校验）。
        // RETURNING 按 VALUES 顺序返回每行 DB 实际分配的 id，直接按下标配对，PG/SQLite 均正确。
        let inserted = match task::Entity::insert_many(task_models)
            .exec_with_returning(db)
            .await
        {
            Ok(models) => models,
            Err(e) => {
                error!(target: "crontab", error = %e, "batch task insert error");
                return;
            }
        };

        // 从 RETURNING 的 Models 派生每条任务的派发三元组（真实 id + uuid + token）。
        // inserted 与 uuids 同序（写入顺序 == VALUES 顺序 == RETURNING 顺序）。
        let dispatch = pair_task_ids(&inserted);

        // 并发发送任务事件（dispatch 中每个元组的 task_id 均为 DB 真实 id）
        let manager = TaskManager::global();
        let mut send_set = JoinSet::new();

        for (task_id, uuid, token) in dispatch {
            let task = TaskEvent {
                task_id,
                task_token: token,
                task_event_type: task_event_type.clone(),
            };
            send_set.spawn(async move {
                (uuid, task_id.cast_signed(), manager.send_event(uuid, task).await)
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

/// 从 `insert_many(...).exec_with_returning()` 返回的 Models 派生每条任务的派发三元组：
/// DB 真实 id、目标 agent uuid、token。
///
/// 关键不变量：每个 `model.id` 是 DB 实际分配的值，**不假设** id 在批次内连续。
/// PostgreSQL 的 IDENTITY 序列在并发 `insert_many` 下会按行交错分配 id（如本批得
/// `100/102/104`），故绝不能用 `last_insert_id - (n-1)` 反推区间——必须用 RETURNING
/// 的真实 id 按写入顺序配对。
///
/// RETURNING 行顺序 == VALUES 写入顺序 == `task_models` 构建顺序 == `uuids` 顺序，
/// 因此 `dispatch[i]` 对应 `uuids[i]`。
fn pair_task_ids(models: &[task::Model]) -> Vec<(u64, Uuid, String)> {
    models
        .iter()
        .map(|m| (m.id.cast_unsigned(), m.uuid, m.token.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::pair_task_ids;
    use ng_db::entity::task;
    use sea_orm::entity::prelude::*;
    use uuid::Uuid;

    /// 构造一个最小可用的 task::Model，仅关心测试用到的字段（id/uuid/token）。
    fn make_model(id: i64, uuid: Uuid, token: &str) -> task::Model {
        task::Model {
            id,
            uuid,
            token: token.to_owned(),
            cron_source: None,
            timestamp: None,
            success: None,
            error_message: None,
            task_event_type: Json::Null,
            task_event_result: None,
        }
    }

    /// 复刻 PostgreSQL 并发交错场景：本批 RETURNING 回来的 id 非连续（100/102/104）。
    /// 旧的反推逻辑 `base_id = last_id - (n-1)` 在此输入下会算出 102/103/104，
    /// 把属于别的批次的 103 当作本批任务下发；本测试证明修复后配对与真实 id 一一对应。
    #[test]
    fn pair_task_ids_interleaved_non_contiguous_ids() {
        let u1 = Uuid::nil();
        let u2 = Uuid::from_u128(2);
        let u3 = Uuid::from_u128(3);
        let models = vec![
            make_model(100, u1, "t1"),
            make_model(102, u2, "t2"),
            make_model(104, u3, "t3"),
        ];

        let dispatch = pair_task_ids(&models);

        assert_eq!(dispatch.len(), 3);
        // 每个 task_id 必须等于对应 model 的真实 id（不连续也正确配对）
        assert_eq!(dispatch[0], (100, u1, "t1".to_owned()));
        assert_eq!(dispatch[1], (102, u2, "t2".to_owned()));
        assert_eq!(dispatch[2], (104, u3, "t3".to_owned()));
    }

    /// 返回顺序与输入顺序严格一致（写入顺序 == 下发顺序，不发生移位）。
    #[test]
    fn pair_task_ids_preserves_order() {
        let ids = [7_i64, 3, 12, 1, 9];
        let models: Vec<task::Model> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| make_model(id, Uuid::from_u128(i as u128), &format!("tok{i}")))
            .collect();

        let dispatch = pair_task_ids(&models);

        assert_eq!(
            dispatch.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            vec![7_u64, 3, 12, 1, 9]
        );
        assert_eq!(
            dispatch
                .iter()
                .map(|(_, _, t)| t.as_str())
                .collect::<Vec<_>>(),
            vec!["tok0", "tok1", "tok2", "tok3", "tok4"]
        );
    }

    #[test]
    fn pair_task_ids_empty() {
        assert!(pair_task_ids(&[]).is_empty());
    }

    #[test]
    fn pair_task_ids_single() {
        let uuid = Uuid::nil();
        let models = vec![make_model(42, uuid, "solo")];
        let dispatch = pair_task_ids(&models);
        assert_eq!(dispatch, vec![(42, uuid, "solo".to_owned())]);
    }

    /// 长度守恒：N 个 model → N 个元组。
    #[test]
    fn pair_task_ids_length_conservation() {
        for n in [1_usize, 2, 5, 50] {
            let models: Vec<task::Model> = (0..n)
                .map(|i| make_model(i as i64, Uuid::from_u128(i as u128), "x"))
                .collect();
            assert_eq!(pair_task_ids(&models).len(), n);
        }
    }

    /// SQLite 端到端回归测试：`insert_many(...).exec_with_returning()` 在 SQLite 下必须成功
    /// 返回每行真实 Model，而非 `BackendNotSupported`。
    ///
    /// 背景：sea-orm 的 `exec_with_returning` 仅在 `sqlite-use-returning-for-3_35` feature 启用
    /// 时对 SQLite 走 RETURNING；未启用则直接返回 `Err(BackendNotSupported { Sqlite })`，导致
    /// `crontab_task` 在 SQLite（文档化默认后端）下静默丢弃整批任务。本测试用内存 SQLite 实跑，
    /// 锁定该 feature 必须启用——若有人误删 Cargo.toml 里的 feature，此测试会失败。
    /// （之前的纯函数测试不连 DB，测不到这条 sea-orm 路径，是该 blocker 漏网的原因。）
    #[tokio::test]
    async fn insert_many_exec_with_returning_works_on_sqlite() {
        use sea_orm::{ActiveValue, ConnectionTrait, Database, FromQueryResult, Set, Statement};

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");

        // 运行时 SQLite 版本必须 ≥ 3.35（RETURNING 子句最低要求）。
        // sqlx 的 `sqlite` feature 默认带 bundled，理论上捆绑 3.50.x；此处实跑确认，
        // 排除「链接到系统旧版 SQLite < 3.35 导致 RETURNING 运行时失败」的风险。
        #[derive(FromQueryResult)]
        struct VersionRow {
            v: String,
        }
        let ver = VersionRow::find_by_statement(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT sqlite_version() AS v",
            [],
        ))
        .one(&db)
        .await
        .expect("query sqlite_version")
        .expect("version row");
        let (major, minor) = {
            let mut it = ver.v.split('.');
            (
                it.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
                it.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
            )
        };
        let ver_str = ver.v.clone();
        assert!(
            (major, minor) >= (3, 35),
            "运行时 SQLite {ver_str} < 3.35，RETURNING 不可用；确认启用了 sqlx bundled",
        );

        // 建最小 task 表（列对齐 task entity；id 自增主键）
        db.execute_unprepared(
            r#"CREATE TABLE "task" (
                "id" integer PRIMARY KEY AUTOINCREMENT,
                "uuid" blob NOT NULL,
                "token" text NOT NULL,
                "cron_source" text,
                "timestamp" bigint,
                "success" boolean,
                "error_message" text,
                "task_event_type" blob NOT NULL,
                "task_event_result" blob
            )"#,
        )
        .await
        .expect("create task table");

        // 批量插入 50 行（较大批量，排除「批量+RETURNING」的驱动边角 bug），用 RETURNING 拿回真实 Model
        const N: i64 = 50;
        let models: Vec<task::ActiveModel> = (0..N)
            .map(|i| task::ActiveModel {
                id: ActiveValue::default(),
                uuid: Set(Uuid::from_u128(i as u128)),
                token: Set(format!("tok{i}")),
                cron_source: Set(None),
                timestamp: Set(None),
                success: Set(None),
                error_message: Set(None),
                task_event_type: Set(serde_json::Value::Null),
                task_event_result: Set(None),
            })
            .collect();

        let inserted = task::Entity::insert_many(models)
            .exec_with_returning(&db)
            .await
            .expect("exec_with_returning must succeed on SQLite (sqlite-use-returning-for-3_35 feature)");

        // 返回行数 == 插入数；id 由 DB 分配（连续与否不重要，关键是真实回读）
        assert_eq!(inserted.len() as i64, N, "RETURNING 应返回全部 {N} 行");
        let ids: Vec<i64> = inserted.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            (1..=N).collect::<Vec<_>>(),
            "SQLite 单条多值 INSERT rowid 连续递增"
        );
        // token 顺序与写入一致（dispatch[i] ↔ uuids[i] 配对正确性）
        assert_eq!(
            inserted
                .iter()
                .map(|m| m.token.as_str())
                .collect::<Vec<_>>(),
            (0..N).map(|i| format!("tok{i}")).collect::<Vec<_>>()
        );
    }
}
