mod server_cron;
mod task;

use crate::DB;
use crate::db_connection::clean_up::cleanup_expired_data;
use crate::entity::{crontab, crontab_result};
use crate::rpc::js_worker::service::enqueue_defined_js_worker_run;
use chrono::{TimeZone, Utc};
use cron::Schedule;
use log::info;
use log::{error, warn};
use nodeget_lib::crontab::{AgentCronType, Cron, CronType, ServerCronType};
use nodeget_lib::js_runtime::RunType;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, Set};
use sea_orm::{EntityTrait, QueryFilter};
use serde_json::Value;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

pub async fn delete_crontab_by_name(name: String) -> Result<bool, sea_orm::DbErr> {
    let db = DB.get().ok_or_else(|| {
        sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_string(),
        ))
    })?;

    crontab::Entity::delete_many()
        .filter(crontab::Column::Name.eq(name))
        .exec(db)
        .await
        .map(|result| result.rows_affected > 0)
}

pub async fn set_crontab_enable_by_name(
    name: String,
    enable: bool,
) -> Result<Option<bool>, sea_orm::DbErr> {
    let db = DB.get().ok_or_else(|| {
        sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
            "Database not initialized".to_string(),
        ))
    })?;

    let crontab_option = crontab::Entity::find()
        .filter(crontab::Column::Name.eq(&name))
        .one(db)
        .await?;

    match crontab_option {
        Some(model) => {
            let mut active_model: crontab::ActiveModel = model.into();
            active_model.enable = Set(enable);
            let updated = active_model.update(db).await?;
            Ok(Some(updated.enable))
        }
        None => Ok(None),
    }
}

pub fn init_crontab_worker() {
    static CRONTAB_WORKER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if CRONTAB_WORKER_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        info!("Crontab scheduler started.");
        loop {
            sleep(Duration::from_secs(1)).await;

            tokio::spawn(async move {
                process_crontab().await;
            });
        }
    });
}

async fn process_crontab() {
    let Some(db) = DB.get() else {
        error!("DB not initialized");
        return;
    };

    let jobs = match crontab::Entity::find()
        .filter(crontab::Column::Enable.eq(true))
        .all(db)
        .await
    {
        Ok(jobs) => jobs,
        Err(err) => {
            error!("{err}");
            return;
        }
    };

    let now = Utc::now();

    for job in jobs {
        let schedule = match Schedule::from_str(&job.cron_expression) {
            Ok(s) => s,
            Err(e) => {
                warn!("Invalid cron expression for job {}: {}", job.id, e);
                continue;
            }
        };

        let last_run = job.last_run_time.map_or_else(
            || now - chrono::Duration::seconds(1),
            |t| Utc.timestamp_millis_opt(t).unwrap(),
        );

        let should_run = schedule
            .after(&last_run)
            .next()
            .is_some_and(|next_run| next_run <= now);

        if !should_run {
            continue;
        }

        info!("Triggering cron job: {} ({})", job.name, job.id);

        let active_model = crontab::ActiveModel {
            id: Set(job.id),
            last_run_time: Set(Some(now.timestamp_millis())),
            ..Default::default()
        };
        if let Err(e) = active_model.update(db).await {
            error!("Failed to update last_run_time for job {}: {}", job.id, e);
            continue;
        }

        let cron_type = serde_json::from_str(&format!("{}", job.cron_type))
            .map_err(|e| warn!("Invalid cron type for job {}: {}", job.id, e))
            .ok();

        let Some(cron_type) = cron_type else { continue };

        let job_parsed = Cron {
            id: job.id,
            name: job.name,
            enable: job.enable,
            cron_expression: job.cron_expression,
            cron_type,
            last_run_time: job.last_run_time,
        };

        tokio::spawn(async move {
            run_job_logic(job_parsed).await;
        });
    }
}

async fn run_job_logic(job: Cron) {
    match job.cron_type {
        CronType::Agent(uuids, AgentCronType::Task(task_event_type)) => {
            task::crontab_task(job.id, job.name, uuids, task_event_type).await;
        }

        CronType::Server(ServerCronType::CleanUpDatabase) => {
            run_cleanup_database_job(job.id, job.name).await;
        }
        CronType::Server(ServerCronType::JsWorker(js_script_name, params)) => {
            run_js_worker_job(job.id, job.name, js_script_name, params).await;
        }
    }
}

/// 运行数据库清理任务并记录结果
async fn run_cleanup_database_job(cron_id: i64, cron_name: String) {
    let Some(db) = DB.get() else {
        error!("DB not initialized for cleanup job [{cron_name}]");
        return;
    };

    // 执行清理
    let (success, message) = match cleanup_expired_data().await {
        Ok(result) => {
            let msg = format!(
                "数据库清理完成。已删除：static_monitoring={}，dynamic_monitoring={}，task={}，crontab_result={}",
                result.static_monitoring_deleted,
                result.dynamic_monitoring_deleted,
                result.task_deleted,
                result.crontab_result_deleted
            );
            info!("{msg}");
            (true, msg)
        }
        Err(e) => {
            let msg = format!("数据库清理失败：{e}");
            error!("{msg}");
            (false, msg)
        }
    };

    // 记录执行结果到 crontab_result
    let crontab_log = crontab_result::ActiveModel {
        id: ActiveValue::NotSet,
        cron_id: Set(cron_id),
        cron_name: Set(cron_name.clone()),
        special_id: Set(None),
        run_time: Set(Some(Utc::now().timestamp_millis())),
        success: Set(Some(success)),
        message: Set(Some(message)),
    };

    if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
        error!("Failed to save CrontabResult for cleanup job [{cron_name}]: {e}");
    }
}

async fn run_js_worker_job(cron_id: i64, cron_name: String, js_script_name: String, params: Value) {
    let db = match DB.get() {
        Some(db) => db,
        None => {
            error!("DB 未初始化，无法执行 JsWorker Cron [{}]", cron_name);
            return;
        }
    };

    let run_result =
        enqueue_defined_js_worker_run(js_script_name.clone(), RunType::Cron, params, None).await;

    let (success, message, special_id) = match run_result {
        Ok(id) => (
            true,
            format!(
                "已触发 JsWorker 定时任务，脚本名：{}，special_id：{}",
                js_script_name, id
            ),
            Some(id),
        ),
        Err(e) => (
            false,
            format!("触发 JsWorker 定时任务失败，脚本名：{}，错误：{}", js_script_name, e),
            None,
        ),
    };

    let crontab_log = crontab_result::ActiveModel {
        id: ActiveValue::NotSet,
        cron_id: Set(cron_id),
        cron_name: Set(cron_name.clone()),
        special_id: Set(special_id),
        run_time: Set(Some(Utc::now().timestamp_millis())),
        success: Set(Some(success)),
        message: Set(Some(message)),
    };

    if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
        error!("Failed to save CrontabResult for js_worker job [{cron_name}]: {e}");
    }
}
