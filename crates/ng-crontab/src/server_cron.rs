//! Server 端定时任务调度循环：每分钟检测到期任务并触发执行。
//!
//! 启动时通过 [`init_crontab_worker`] 注册一个 tokio 协程，
//! 协程对齐分钟边界睡眠，唤醒后遍历缓存中所有已启用的定时任务，
//! 判断是否到期触发。Agent 类型走 Task 下发，Server 类型走 JS Worker。
//! 同时提供按名称删除和启用/禁用的辅助函数。

use crate::cache::CrontabCache;
use crate::task::js_worker_scheduler;
use crate::{AgentCronType, Cron, CronType, ServerCronType};
use chrono::{DateTime, TimeZone, Utc};
use ng_db::entity::{crontab, crontab_result};
use ng_db::get_db;
use ng_js_runtime::RunType;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tracing::{Instrument, debug, error, info, info_span, warn};

/// 用于在 crontab 配置变更（create/edit/delete/set_enable）后提前唤醒调度器，
/// 使其立即重算最近触发时刻，而非等待下一次定时唤醒。
static CRONTAB_RELOAD_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

/// 获取全局 crontab 调度唤醒 Notify（懒初始化）。
fn reload_notify() -> &'static Arc<Notify> {
    CRONTAB_RELOAD_NOTIFY.get_or_init(|| Arc::new(Notify::new()))
}

/// 通知调度器配置已变更，提前唤醒以重算最近触发时刻。
/// 供 create/edit/delete/set_enable 在增量更新缓存后调用。
pub fn notify_crontab_changed() {
    reload_notify().notify_one();
}

/// 按名称删除定时任务，并刷新缓存。
///
/// - `name` - 定时任务名称
/// - 返回是否成功删除（false 表示未找到该名称的任务）
pub async fn delete_crontab_by_name(name: String) -> Result<bool, sea_orm::DbErr> {
    debug!(target: "crontab", name = %name, "deleting crontab");
    let db = get_db().ok_or_else(|| {
        sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_string(),
        ))
    })?;

    let result = crontab::Entity::delete_many()
        .filter(crontab::Column::Name.eq(&name))
        .exec(db)
        .await?;

    let deleted = result.rows_affected > 0;
    if deleted {
        info!(target: "crontab", name = %name, "crontab deleted");
        // 增量移除缓存中该 name 的条目，替代全量 reload
        if let Some(cache) = CrontabCache::global() {
            cache.remove_by_name(&name);
            notify_crontab_changed();
        } else if let Err(e) = CrontabCache::reload().await {
            error!(target: "crontab", error = %e, "failed to reload crontab cache after delete");
        }
    } else {
        warn!(target: "crontab", name = %name, "crontab not found for deletion");
    }
    Ok(deleted)
}

/// 按名称设置定时任务的启用/禁用状态，并刷新缓存。
///
/// - `name` - 定时任务名称
/// - `enable` - 目标启用状态
/// - 返回 Some(enable) 表示更新成功，None 表示未找到该任务
pub async fn set_crontab_enable_by_name(
    name: String,
    enable: bool,
) -> Result<Option<bool>, sea_orm::DbErr> {
    debug!(target: "crontab", name = %name, enable = enable, "setting crontab enable");
    let db = get_db().ok_or_else(|| {
        sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_string(),
        ))
    })?;

    let crontab_option = crontab::Entity::find()
        .filter(crontab::Column::Name.eq(&name))
        .one(db)
        .await?;

    if let Some(model) = crontab_option {
        let mut active_model: crontab::ActiveModel = model.into();
        active_model.enable = Set(enable);
        let updated = active_model.update(db).await?;
        info!(target: "crontab", name = %name, enable = updated.enable, "crontab enable updated");
        // 增量更新缓存（仅解析该条目），替代全量 reload
        if let Some(cache) = CrontabCache::global() {
            cache.upsert(updated.clone());
            notify_crontab_changed();
        } else if let Err(e) = CrontabCache::reload().await {
            error!(target: "crontab", error = %e, "failed to reload crontab cache after set_enable");
        }
        Ok(Some(updated.enable))
    } else {
        warn!(target: "crontab", name = %name, enable, "crontab not found for set_enable");
        Ok(None)
    }
}

/// 保证调度协程只启动一次的标记。
static CRONTAB_WORKER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// 初始化定时任务调度协程（全局只启动一次）。
///
/// 调度器计算所有已启用任务的最近触发时刻，`sleep_until` 到该时刻唤醒处理，
/// 替代原先每秒轮询（cron 最小粒度通常分钟，60× 冗余）。配置变更
/// （create/edit/delete/set_enable）会通过 `notify_crontab_changed` 提前唤醒重算。
/// 设 60 秒上限防止无任务或下次触发很远时睡死。
pub fn init_crontab_worker() {
    info!(target: "crontab", "initializing crontab worker");
    if CRONTAB_WORKER_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        info!(target: "crontab", "scheduler started");
        // 启动时先处理一次（与原行为一致：启动后短延迟即检查），再进入 sleep_until 循环
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            process_crontab().await;
            let next_deadline = compute_next_deadline();
            // select: 等到下次触发时刻，或配置变更通知提前唤醒
            tokio::select! {
                _ = tokio::time::sleep_until(next_deadline) => {}
                _ = reload_notify().notified() => {
                    debug!(target: "crontab", "scheduler woken early by config change");
                }
            }
        }
    });
}

/// 计算调度器下次应唤醒的时刻（`tokio::time::Instant`）。
///
/// 遍历所有已启用任务的最近触发点取 min；无任务或下次触发超过 60 秒时，
/// 返回 60 秒后（上限，保证周期性自检 + 缓存一致性兜底）。
fn compute_next_deadline() -> tokio::time::Instant {
    let now = Utc::now();
    let now_instant = tokio::time::Instant::now();
    let cap = Duration::from_secs(60);

    let Some(cache) = CrontabCache::global() else {
        return now_instant + cap;
    };
    let jobs = cache.get_enabled_entries();

    let mut earliest: Option<DateTime<Utc>> = None;
    for entry in &jobs {
        let effective_last = cache.get_last_run_time(entry.model.id, entry.model.last_run_time);
        let last_run = effective_last.map_or_else(
            || now - chrono::Duration::seconds(1),
            |t| Utc.timestamp_millis_opt(t).single().unwrap_or(now),
        );
        if let Some(next_run) = entry.schedule.after(&last_run).next() {
            earliest = Some(match earliest {
                None => next_run,
                Some(e) if next_run < e => next_run,
                Some(e) => e,
            });
        }
    }

    let earliest = earliest.unwrap_or(now + chrono::Duration::seconds(60));
    // 转为 Duration：若 next_run 已过（应立即触发），sleep 极短即可
    let delta_ms = std::cmp::max((earliest - now).num_milliseconds(), 0i64) as u64;
    let capped = std::cmp::min(delta_ms, cap.as_millis() as u64);
    now_instant + Duration::from_millis(capped)
}

/// 单次调度处理：遍历已启用的定时任务，判断是否到期触发。
///
/// 1. 从缓存获取所有已启用条目
/// 2. 对每个条目计算上次运行时间与下次触发时间
/// 3. 若触发时间 <= 当前时间，则标记已运行并 spawn 异步执行
/// 4. 等待所有 spawn 的任务完成
async fn process_crontab() {
    debug!(target: "crontab", "processing crontab tick");
    let Some(db) = get_db() else {
        error!(target: "crontab", "DB not initialized");
        return;
    };

    let Some(cache) = CrontabCache::global() else {
        error!(target: "crontab", "CrontabCache not initialized");
        return;
    };
    let jobs = cache.get_enabled_entries();

    let now = Utc::now();
    let now_millis = now.timestamp_millis();

    // 第一阶段：遍历判断哪些任务到期，收集待触发任务。
    // 不在循环内逐个 update DB（原实现 N 个到期任务 = N 次串行 round-trip）。
    #[allow(clippy::too_many_lines)]
    let mut due: Vec<(Cron, i64)> = Vec::new();
    for entry in &jobs {
        // 获取有效的 last_run_time：优先覆盖映射，回退到数据库值
        let effective_last = cache.get_last_run_time(entry.model.id, entry.model.last_run_time);
        // 将毫秒时间戳转换为 DateTime，无效时间戳回退到 epoch
        let last_run = effective_last.map_or_else(
            // 从未运行过的任务视为"1 秒前运行"，确保首次调度能触发
            || now - chrono::Duration::seconds(1),
            |t| {
                Utc.timestamp_millis_opt(t).single().unwrap_or_else(|| {
                    warn!(target: "crontab", t, "Invalid last_run_time, treating as never run");
                    Utc.timestamp_millis_opt(0)
                        .single()
                        .unwrap_or_else(Utc::now)
                })
            },
        );

        // 判断是否应触发：上次运行后是否存在 <= 当前时间的下一次触发点
        let should_run = entry
            .schedule
            .after(&last_run)
            .next()
            .is_some_and(|next_run| next_run <= now);

        if !should_run {
            continue;
        }

        info!(
            target: "crontab",
            job_id = entry.model.id,
            job_name = %entry.model.name,
            cron_expression = %entry.model.cron_expression,
            "triggering cron job"
        );

        let job_parsed = Cron {
            id: entry.model.id,
            name: entry.model.name.clone(),
            enable: entry.model.enable,
            cron_expression: entry.model.cron_expression.clone(),
            cron_type: entry.cron_type.clone(),
            last_run_time: effective_last,
        };

        due.push((job_parsed, entry.model.id));
    }

    // 第二阶段：批量更新所有到期任务的 last_run_time（单条 UPDATE ... WHERE id IN (...)），
    // 替代原 for 循环内逐个 update 的 N 次串行 round-trip。所有到期任务共享同一 now_millis。
    if !due.is_empty() {
        let due_ids: Vec<i64> = due.iter().map(|(_, id)| *id).collect();
        let update_result = crontab::Entity::update_many()
            .filter(crontab::Column::Id.is_in(due_ids))
            .col_expr(crontab::Column::LastRunTime, now_millis.into())
            .exec(db)
            .await;
        match update_result {
            Ok(_) => {
                // 批量更新成功，同步更新缓存覆盖映射（per-id，开销小）
                for (_, id) in &due {
                    cache.update_last_run_time(*id, now_millis);
                }
            }
            Err(e) => {
                error!(target: "crontab", error = %e, "failed to batch update last_run_time in DB");
            }
        }
    }

    // 第三阶段：spawn 执行所有到期任务
    let mut set = JoinSet::new();
    for (job_parsed, job_id) in due {
        let job_name = job_parsed.name.clone();
        let span = info_span!(
            target: "crontab",
            "crontab::run_job",
            job_id,
            job_name = %job_name,
        );
        set.spawn(
            async move {
                run_job_logic(job_parsed).await;
                debug!(target: "crontab", "cron job completed");
            }
            .instrument(span),
        );
    }

    // 等待所有 spawn 的任务完成，捕获 panic
    while let Some(res) = set.join_next().await {
        if let Err(e) = res {
            error!(target: "crontab", error = %e, "cron job panicked");
        }
    }
}

/// 根据 CronType 分发任务执行逻辑。
///
/// - Agent 类型：调用 [`crate::task::crontab_task`] 下发任务
/// - Server 类型：调用 [`run_js_worker_job`] 执行 JS Worker 脚本
async fn run_job_logic(job: Cron) {
    debug!(target: "crontab", job_name = %job.name, job_type = ?job.cron_type, "dispatching cron job");
    match job.cron_type {
        CronType::Agent(uuids, AgentCronType::Task(task_event_type)) => {
            let agent_count = uuids.len();
            info!(
                target: "crontab",
                agent_count,
                task_type = ?task_event_type,
                "dispatching agent task"
            );
            crate::task::crontab_task(job.id, job.name, uuids, task_event_type).await;
        }

        CronType::Server(ServerCronType::JsWorker(js_script_name, params)) => {
            info!(
                target: "crontab",
                js_script_name = %js_script_name,
                "running js_worker job"
            );
            run_js_worker_job(job.id, job.name, js_script_name, params).await;
        }
    }
}

/// 执行 JS Worker 类型的定时任务。
///
/// 1. 通过 `JsWorkerScheduler` 提交脚本运行请求
/// 2. 根据执行结果构建 CrontabResult 记录
/// 3. 将结果插入 crontab_result 表
async fn run_js_worker_job(
    cron_id: i64,
    cron_name: String,
    js_script_name: String,
    params: serde_json::Value,
) {
    info!(target: "crontab", cron_id = cron_id, cron_name = %cron_name, js_script_name = %js_script_name, "running js worker cron job");
    let Some(db) = get_db() else {
        error!(
            target: "crontab",
            cron_id,
            cron_name = %cron_name,
            js_script_name = %js_script_name,
            "DB not initialized for js_worker job"
        );
        return;
    };

    let run_result = match js_worker_scheduler() {
        Some(scheduler) => {
            scheduler
                .enqueue_run(js_script_name.clone(), RunType::Cron, params, None)
                .await
        }
        None => Err(anyhow::anyhow!("JsWorkerScheduler not initialized")),
    };

    // 根据调度结果构建状态信息
    let (success, message, relative_id) = match run_result {
        Ok(id) => {
            info!(
                target: "crontab",
                cron_id,
                cron_name = %cron_name,
                js_script_name = %js_script_name,
                relative_id = id,
                "js_worker cron job triggered"
            );
            (
                true,
                format!("已触发 JsWorker 定时任务，脚本名：{js_script_name}，relative_id：{id}"),
                Some(id),
            )
        }
        Err(e) => {
            error!(
                target: "crontab",
                cron_id,
                cron_name = %cron_name,
                js_script_name = %js_script_name,
                error = %e,
                "js_worker cron job trigger failed"
            );
            (
                false,
                format!("触发 JsWorker 定时任务失败，脚本名：{js_script_name}，错误：{e}"),
                None,
            )
        }
    };

    // 写入执行结果记录
    let crontab_log = crontab_result::ActiveModel {
        id: ActiveValue::NotSet,
        cron_id: Set(cron_id),
        cron_name: Set(cron_name.clone()),
        relative_id: Set(relative_id),
        run_time: Set(Some(Utc::now().timestamp_millis())),
        success: Set(Some(success)),
        message: Set(Some(message)),
    };

    if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
        error!(
            target: "crontab",
            cron_id,
            cron_name = %cron_name,
            error = %e,
            "failed to save crontab_result for js_worker job"
        );
    }
}
