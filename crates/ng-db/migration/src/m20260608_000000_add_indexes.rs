use crate::sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

/// 新增数据库索引以加速常见查询路径。
///
/// 新增索引：
/// - crontab_result: cron_id, cron_name, run_time DESC
/// - js_result: js_worker_id, (js_worker_name, start_time DESC)
/// - dynamic_monitoring: storage_time
/// - dynamic_monitoring_summary: storage_time
/// - static_monitoring: storage_time
/// - task: task_event_type (SQLite 普通索引; PostgreSQL 使用 GIN 加速 JSON 查询)
///
/// 冗余索引备注（应在后续迁移中移除）：
/// - idx-crontab-name: 重复了 crontab.name 的 UNIQUE 约束（列定义已带 unique_key）
/// - idx-js_worker-id: 重复了 js_worker.id 的 PRIMARY KEY
/// - idx-crontab_result-id: 重复了 crontab_result.id 的 PRIMARY KEY
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // crontab_result: cron_id
        manager
            .create_index(
                Index::create()
                    .name("idx-crontab_result-cron_id")
                    .table(CrontabResult::Table)
                    .col(CrontabResult::CronId)
                    .to_owned(),
            )
            .await?;

        // crontab_result: cron_name
        manager
            .create_index(
                Index::create()
                    .name("idx-crontab_result-cron_name")
                    .table(CrontabResult::Table)
                    .col(CrontabResult::CronName)
                    .to_owned(),
            )
            .await?;

        // crontab_result: run_time DESC
        // SeaORM 的 Index::create 不直接支持排序，使用原始 SQL
        match manager.get_database_backend() {
            DbBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"CREATE INDEX IF NOT EXISTS "idx-crontab_result-run_time" ON "crontab_result" ("run_time" DESC)"#,
                    )
                    .await?;
            }
            DbBackend::Sqlite => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"CREATE INDEX IF NOT EXISTS "idx-crontab_result-run_time" ON "crontab_result" ("run_time" DESC)"#,
                    )
                    .await?;
            }
            _ => {}
        }

        // js_result: js_worker_id
        manager
            .create_index(
                Index::create()
                    .name("idx-js_result-js_worker_id")
                    .table(JsResult::Table)
                    .col(JsResult::JsWorkerId)
                    .to_owned(),
            )
            .await?;

        // js_result: (js_worker_name, start_time DESC)
        match manager.get_database_backend() {
            DbBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"CREATE INDEX IF NOT EXISTS "idx-js_result-js_worker_name-start_time" ON "js_result" ("js_worker_name", "start_time" DESC)"#,
                    )
                    .await?;
            }
            DbBackend::Sqlite => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"CREATE INDEX IF NOT EXISTS "idx-js_result-js_worker_name-start_time" ON "js_result" ("js_worker_name", "start_time" DESC)"#,
                    )
                    .await?;
            }
            _ => {}
        }

        // dynamic_monitoring: storage_time
        manager
            .create_index(
                Index::create()
                    .name("idx-dynamic_monitoring-storage_time")
                    .table(DynamicMonitoring::Table)
                    .col(DynamicMonitoring::StorageTime)
                    .to_owned(),
            )
            .await?;

        // dynamic_monitoring_summary: storage_time
        manager
            .create_index(
                Index::create()
                    .name("idx-dynamic_monitoring_summary-storage_time")
                    .table(DynamicMonitoringSummary::Table)
                    .col(DynamicMonitoringSummary::StorageTime)
                    .to_owned(),
            )
            .await?;

        // static_monitoring: storage_time
        manager
            .create_index(
                Index::create()
                    .name("idx-static_monitoring-storage_time")
                    .table(StaticMonitoring::Table)
                    .col(StaticMonitoring::StorageTime)
                    .to_owned(),
            )
            .await?;

        // task: task_event_type
        // SQLite: 普通索引; PostgreSQL: GIN 索引加速 JSON 查询
        match manager.get_database_backend() {
            DbBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"CREATE INDEX IF NOT EXISTS "idx-task-task_event_type" ON "task" USING GIN ("task_event_type")"#,
                    )
                    .await?;
            }
            DbBackend::Sqlite => {
                manager
                    .create_index(
                        Index::create()
                            .name("idx-task-task_event_type")
                            .table(Task::Table)
                            .col(Task::TaskEventType)
                            .to_owned(),
                    )
                    .await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 按相反顺序删除索引
        match manager.get_database_backend() {
            DbBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(r#"DROP INDEX IF EXISTS "idx-task-task_event_type""#)
                    .await?;
            }
            DbBackend::Sqlite => {
                manager
                    .get_connection()
                    .execute_unprepared(r#"DROP INDEX IF EXISTS "idx-task-task_event_type""#)
                    .await?;
            }
            _ => {}
        }

        manager
            .drop_index(
                Index::drop()
                    .name("idx-static_monitoring-storage_time")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx-dynamic_monitoring_summary-storage_time")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx-dynamic_monitoring-storage_time")
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres | DbBackend::Sqlite => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"DROP INDEX IF EXISTS "idx-js_result-js_worker_name-start_time""#,
                    )
                    .await?;
            }
            _ => {}
        }

        manager
            .drop_index(
                Index::drop()
                    .name("idx-js_result-js_worker_id")
                    .to_owned(),
            )
            .await?;

        match manager.get_database_backend() {
            DbBackend::Postgres | DbBackend::Sqlite => {
                manager
                    .get_connection()
                    .execute_unprepared(r#"DROP INDEX IF EXISTS "idx-crontab_result-run_time""#)
                    .await?;
            }
            _ => {}
        }

        manager
            .drop_index(
                Index::drop()
                    .name("idx-crontab_result-cron_name")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx-crontab_result-cron_id")
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum CrontabResult {
    #[sea_orm(iden = "crontab_result")]
    Table,
    CronId,
    CronName,
}

#[derive(DeriveIden)]
enum JsResult {
    #[sea_orm(iden = "js_result")]
    Table,
    JsWorkerId,
}

#[derive(DeriveIden)]
enum DynamicMonitoring {
    #[sea_orm(iden = "dynamic_monitoring")]
    Table,
    StorageTime,
}

#[derive(DeriveIden)]
enum DynamicMonitoringSummary {
    #[sea_orm(iden = "dynamic_monitoring_summary")]
    Table,
    StorageTime,
}

#[derive(DeriveIden)]
enum StaticMonitoring {
    #[sea_orm(iden = "static_monitoring")]
    Table,
    StorageTime,
}

#[derive(DeriveIden)]
enum Task {
    #[sea_orm(iden = "task")]
    Table,
    TaskEventType,
}
