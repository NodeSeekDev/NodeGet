use crate::query::{CrontabResultDataQuery, CrontabResultQueryCondition};
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_db::entity::crontab_result;
use ng_db::get_db;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn delete(token: String, query: CrontabResultDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab_result", condition_count = query.condition.len(), "processing crontab_result delete request");
        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

        // 权限检查 —— 与 query 相同的模式
        let mut permission_checked = false;
        let mut has_cron_name_filter = false;
        let condition_count = query.condition.len();

        for condition in &query.condition {
            if let CrontabResultQueryCondition::CronName(cron_name) = condition {
                if !permission_checked {
                    super::auth::check_crontab_result_delete_permission(&token, Some(cron_name))
                        .await?;
                    permission_checked = true;
                }
                has_cron_name_filter = true;
            }
        }

        if !has_cron_name_filter && !permission_checked {
            super::auth::check_crontab_result_delete_permission(&token, None).await?;
        }

        debug!(target: "crontab_result", condition_count, has_cron_name_filter, "crontab_result delete permission check passed");

        // 同时构建 select_query 与 delete_query，应用完全相同的 condition
        let mut select_query = crontab_result::Entity::find()
            .select_only()
            .column(crontab_result::Column::Id);
        let mut delete_query = crontab_result::Entity::delete_many();
        let mut is_last = false;
        let mut limit_count: Option<u64> = None;

        for condition in query.condition {
            match condition {
                CrontabResultQueryCondition::Id(id) => {
                    select_query = select_query.filter(crontab_result::Column::Id.eq(id));
                    delete_query = delete_query.filter(crontab_result::Column::Id.eq(id));
                }
                CrontabResultQueryCondition::CronId(cron_id) => {
                    select_query = select_query.filter(crontab_result::Column::CronId.eq(cron_id));
                    delete_query = delete_query.filter(crontab_result::Column::CronId.eq(cron_id));
                }
                CrontabResultQueryCondition::CronName(cron_name) => {
                    select_query =
                        select_query.filter(crontab_result::Column::CronName.eq(cron_name.clone()));
                    delete_query =
                        delete_query.filter(crontab_result::Column::CronName.eq(cron_name));
                }
                CrontabResultQueryCondition::RunTimeFromTo(start, end) => {
                    select_query = select_query.filter(
                        crontab_result::Column::RunTime
                            .gte(start)
                            .and(crontab_result::Column::RunTime.lte(end)),
                    );
                    delete_query = delete_query.filter(
                        crontab_result::Column::RunTime
                            .gte(start)
                            .and(crontab_result::Column::RunTime.lte(end)),
                    );
                }
                CrontabResultQueryCondition::RunTimeFrom(start) => {
                    select_query = select_query.filter(crontab_result::Column::RunTime.gte(start));
                    delete_query = delete_query.filter(crontab_result::Column::RunTime.gte(start));
                }
                CrontabResultQueryCondition::RunTimeTo(end) => {
                    select_query = select_query.filter(crontab_result::Column::RunTime.lte(end));
                    delete_query = delete_query.filter(crontab_result::Column::RunTime.lte(end));
                }
                CrontabResultQueryCondition::IsSuccess => {
                    select_query = select_query.filter(crontab_result::Column::Success.eq(true));
                    delete_query = delete_query.filter(crontab_result::Column::Success.eq(true));
                }
                CrontabResultQueryCondition::IsFailure => {
                    select_query = select_query.filter(crontab_result::Column::Success.eq(false));
                    delete_query = delete_query.filter(crontab_result::Column::Success.eq(false));
                }
                CrontabResultQueryCondition::Limit(limit) => {
                    const MAX_LIMIT: u64 = 10_000;
                    limit_count = Some(std::cmp::min(limit, MAX_LIMIT));
                }
                CrontabResultQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        // 有 Limit / Last 时，先 select 出目标 id 再按 id 删除；否则直接 delete_query.exec
        let deleted_rows = if is_last || limit_count.is_some() {
            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = select_query
                .order_by_desc(crontab_result::Column::RunTime)
                .order_by_desc(crontab_result::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    NodegetError::DatabaseError(format!(
                        "Failed to select crontab_result ids for delete: {e}"
                    ))
                })?;

            if ids.is_empty() {
                0
            } else {
                crontab_result::Entity::delete_many()
                    .filter(crontab_result::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        NodegetError::DatabaseError(format!("Failed to delete crontab_result: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            delete_query
                .exec(db)
                .await
                .map_err(|e| {
                    NodegetError::DatabaseError(format!("Failed to delete crontab_result: {e}"))
                })?
                .rows_affected
        };

        let response = serde_json::json!({
            "success": true,
            "deleted": deleted_rows,
            "condition_count": condition_count,
        });

        debug!(target: "crontab_result", deleted_rows, condition_count, "crontab_result delete completed");

        let json_str = serde_json::to_string(&response).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize delete response: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
