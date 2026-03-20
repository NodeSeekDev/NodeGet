mod server_cron;
mod task;

use crate::DB;
use crate::db_connection::clean_up::cleanup_expired_data;
use crate::entity::{crontab, crontab_result};
use chrono::{TimeZone, Utc};
use cron::Schedule;
use log::info;
use log::{error, warn};
use nodeget_lib::crontab::{AgentCronType, Cron, CronType, ServerCronType};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, Set};
use sea_orm::{EntityTrait, QueryFilter};
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

pub async fn toggle_crontab_enable_by_name(name: String) -> Result<Option<bool>, sea_orm::DbErr> {
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
        Some(mut model) => {
            let new_enable = !model.enable;
            model.enable = new_enable;
            let active_model: crontab::ActiveModel = model.into();
            active_model.update(db).await?;
            Ok(Some(new_enable))
        }
        None => Ok(None),
    }
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
        Some(mut model) => {
            model.enable = enable;
            let active_model: crontab::ActiveModel = model.into();
            active_model.update(db).await?;
            Ok(Some(enable))
        }
        None => Ok(None),
    }
}

pub fn init_crontab_worker() {
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

        let mut active_model: crontab::ActiveModel = job.clone().into();
        active_model.last_run_time = Set(Some(now.timestamp_millis()));
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
                "Database cleanup completed. Deleted: static_monitoring={}, dynamic_monitoring={}, task={}, crontab_result={}",
                result.static_monitoring_deleted,
                result.dynamic_monitoring_deleted,
                result.task_deleted,
                result.crontab_result_deleted
            );
            info!("{msg}");
            (true, msg)
        }
        Err(e) => {
            let msg = format!("Database cleanup failed: {e}");
            error!("{msg}");
            (false, msg)
        }
    };

    // 记录执行结果到 crontab_result
    let crontab_log = crontab_result::ActiveModel {
        id: ActiveValue::NotSet,
        cron_id: Set(cron_id),
        cron_name: Set(cron_name.clone()),
        task_id: Set(None),
        run_time: Set(Some(Utc::now().timestamp_millis())),
        success: Set(Some(success)),
        message: Set(Some(message)),
    };

    if let Err(e) = crontab_result::Entity::insert(crontab_log).exec(db).await {
        error!("Failed to save CrontabResult for cleanup job [{cron_name}]: {e}");
    }
}
