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
use tokio::time::sleep;
use tracing::{Instrument, debug, error, info, info_span, warn};

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

static CRONTAB_WORKER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

pub fn init_crontab_worker() {
    info!(target: "crontab", "initializing crontab worker");
    if CRONTAB_WORKER_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        info!(target: "crontab", "scheduler started");
        loop {
            let now = Utc::now();
            let secs = now.timestamp() % 60;
            let wait = if secs == 0 { 60 } else { 60 - secs as u64 };
            sleep(Duration::from_secs(wait)).await;
            process_crontab().await;
            let remaining = 60 - (Utc::now().timestamp() % 60) as u64;
            if remaining > 0 && remaining < 60 {
                sleep(Duration::from_secs(remaining)).await;
            }
        }
    });
}

async fn process_crontab() {
    debug!(target: "crontab", "processing crontab tick");
    let Some(db) = get_db() else {
        error!(target: "crontab", "DB not initialized");
        return;
    };

    let cache = CrontabCache::global();
    let jobs = cache.get_enabled_entries();

    let now = Utc::now();

    let mut set = JoinSet::new();

    for entry in &jobs {
        let last_run = entry.model.last_run_time.map_or_else(
            || now - chrono::Duration::seconds(1),
            |t| Utc.timestamp_millis_opt(t).unwrap(),
        );

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
            last_run_time: entry.model.last_run_time,
        };

        let now_millis = now.timestamp_millis();
        cache.update_last_run_time(entry.model.id, now_millis);

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

    while let Some(res) = set.join_next().await {
        if let Err(e) = res {
            error!(target: "crontab", error = %e, "cron job panicked");
        }
    }
}

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
