use crate::query::{CrontabResultDataQuery, CrontabResultQueryCondition};
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_db::entity::crontab_result;
use ng_db::get_db;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn query(token: String, query: CrontabResultDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab_result", condition_count = query.condition.len(), "processing crontab_result query request");
        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

        // 构建查询
        let mut select = crontab_result::Entity::find();

        // 标记是否已经检查了权限
        let mut permission_checked = false;
        let mut has_cron_name_filter = false;

        // 处理查询条件
        for condition in &query.condition {
            match condition {
                CrontabResultQueryCondition::Id(id) => {
                    select = select.filter(crontab_result::Column::Id.eq(*id));
                }
                CrontabResultQueryCondition::CronId(cron_id) => {
                    select = select.filter(crontab_result::Column::CronId.eq(*cron_id));
                }
                CrontabResultQueryCondition::CronName(cron_name) => {
                    // 检查读权限
                    if !permission_checked {
                        super::auth::check_crontab_result_read_permission(&token, cron_name)
                            .await?;
                        permission_checked = true;
                    }
                    has_cron_name_filter = true;
                    select = select.filter(crontab_result::Column::CronName.eq(cron_name.clone()));
                }
                CrontabResultQueryCondition::RunTimeFromTo(start, end) => {
                    select = select.filter(
                        crontab_result::Column::RunTime
                            .gte(*start)
                            .and(crontab_result::Column::RunTime.lte(*end)),
                    );
                }
                CrontabResultQueryCondition::RunTimeFrom(start) => {
                    select = select.filter(crontab_result::Column::RunTime.gte(*start));
                }
                CrontabResultQueryCondition::RunTimeTo(end) => {
                    select = select.filter(crontab_result::Column::RunTime.lte(*end));
                }
                CrontabResultQueryCondition::IsSuccess => {
                    select = select.filter(crontab_result::Column::Success.eq(true));
                }
                CrontabResultQueryCondition::IsFailure => {
                    select = select.filter(crontab_result::Column::Success.eq(false));
                }
                CrontabResultQueryCondition::Limit(limit) => {
                    const MAX_LIMIT: u64 = 10_000;
                    select = select.limit(std::cmp::min(*limit, MAX_LIMIT));
                }
                CrontabResultQueryCondition::Last => {
                    // 按 run_time 降序排序，只取第一条
                    select = select.order_by_desc(crontab_result::Column::RunTime);
                    select = select.limit(1);
                }
            }
        }

        // 如果没有指定 cron_name 过滤，需要检查全局权限
        if !has_cron_name_filter && !permission_checked {
            super::auth::check_crontab_result_read_permission(&token, "*").await?;
        }

        debug!(target: "crontab_result", condition_count = query.condition.len(), has_cron_name_filter, "crontab_result query permission check passed");

        // 默认按 run_time 降序排序（如果没有指定 Last）
        if !query
            .condition
            .iter()
            .any(|c| matches!(c, CrontabResultQueryCondition::Last))
        {
            select = select.order_by_desc(crontab_result::Column::RunTime);
        }

        // 未显式指定 Limit/Last 时施加默认上限，防止无界查询
        let has_explicit_limit = query.condition.iter().any(|c| {
            matches!(
                c,
                CrontabResultQueryCondition::Limit(_) | CrontabResultQueryCondition::Last
            )
        });
        if !has_explicit_limit {
            select = select.limit(1000);
        }

        // 执行查询
        let results = select.all(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to query crontab_result: {e}"))
        })?;

        debug!(target: "crontab_result", result_count = results.len(), "crontab_result query completed");

        // 序列化结果
        let json_str = serde_json::to_string(&results).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize results: {e}"))
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
