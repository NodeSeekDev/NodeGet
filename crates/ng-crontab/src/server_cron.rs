//! Server 端定时任务调度循环：每分钟检测到期任务并触发执行。
//!
//! 启动时通过 [`init_crontab_worker`] 注册一个 tokio 协程，
//! 协程对齐分钟边界睡眠，唤醒后遍历缓存中所有已启用的定时任务，
//! 判断是否到期触发。Agent 类型走 Task 下发，Server 类型走 JS Worker。
//! 同时提供按名称删除和启用/禁用的辅助函数。

use crate::cache::CrontabCache;
use crate::task::js_worker_scheduler;
use crate::{AgentCronType, Cron, CronType, ServerCronType};
use chrono::{TimeZone, Utc};
use ng_db::entity::{crontab, crontab_result};
use ng_db::get_db;
use ng_js_runtime::RunType;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tracing::{Instrument, debug, error, info, info_span, warn};

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
        if let Err(e) = CrontabCache::reload().await {
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
        if let Err(e) = CrontabCache::reload().await {
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
/// 协程每秒唤醒，从缓存读取已启用任务并判断是否有到期触发。
/// 使用缓存避免每秒查询数据库，仅到期时才写入 DB 更新 last_run_time。
pub fn init_crontab_worker() {
    info!(target: "crontab", "initializing crontab worker");
    if CRONTAB_WORKER_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        info!(target: "crontab", "scheduler started");
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // 消费 interval 的首次立即触发，保持启动时有 1 秒延迟的原始行为
        interval.tick().await;
        loop {
            interval.tick().await;
            process_crontab().await;
        }
    });
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

    let mut set = JoinSet::new();

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

        let job_id = entry.model.id;
        let job_name = entry.model.name.clone();

        let job_parsed = Cron {
            id: entry.model.id,
            name: entry.model.name.clone(),
            enable: entry.model.enable,
            cron_expression: entry.model.cron_expression.clone(),
            cron_type: entry.cron_type.clone(),
            last_run_time: effective_last,
        };

        // 先更新 last_run_time 再 spawn 任务，防止并发调度重复触发
        let now_millis = now.timestamp_millis();

        let active_model = crontab::ActiveModel {
            id: Set(entry.model.id),
            last_run_time: Set(Some(now_millis)),
            ..Default::default()
        };
        if let Err(e) = active_model.update(db).await {
            error!(
                target: "crontab",
                job_id = entry.model.id,
                job_name = %job_name,
                error = %e,
                "failed to update last_run_time in DB"
            );
        } else {
            // 同步更新缓存覆盖映射，保证下次调度使用最新时间戳
            cache.update_last_run_time(entry.model.id, now_millis);
        }

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
