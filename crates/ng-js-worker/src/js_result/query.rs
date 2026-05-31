use crate::auth::{
    JsResultAction, ensure_js_result_permission, resolve_accessible_js_result_workers,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::js_result::query::{JsResultDataQuery, JsResultQueryCondition};
use ng_db::entity::js_result;
use ng_db::get_db;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

fn apply_filter_to_select(
    mut select: sea_orm::Select<js_result::Entity>,
    condition: &JsResultQueryCondition,
) -> sea_orm::Select<js_result::Entity> {
    match condition {
        JsResultQueryCondition::Id(id) => {
            select = select.filter(js_result::Column::Id.eq(*id));
        }
        JsResultQueryCondition::JsWorkerId(js_worker_id) => {
            select = select.filter(js_result::Column::JsWorkerId.eq(*js_worker_id));
        }
        JsResultQueryCondition::JsWorkerName(js_worker_name) => {
            select = select.filter(js_result::Column::JsWorkerName.eq(js_worker_name.clone()));
        }
        JsResultQueryCondition::RunType(run_type) => {
            select = select.filter(js_result::Column::RunType.eq(run_type.clone()));
        }
        JsResultQueryCondition::StartTimeFromTo(start, end) => {
            select = select.filter(
                js_result::Column::StartTime
                    .gte(*start)
                    .and(js_result::Column::StartTime.lte(*end)),
            );
        }
        JsResultQueryCondition::StartTimeFrom(start) => {
            select = select.filter(js_result::Column::StartTime.gte(*start));
        }
        JsResultQueryCondition::StartTimeTo(end) => {
            select = select.filter(js_result::Column::StartTime.lte(*end));
        }
        JsResultQueryCondition::FinishTimeFromTo(start, end) => {
            select = select.filter(
                js_result::Column::FinishTime
                    .gte(*start)
                    .and(js_result::Column::FinishTime.lte(*end)),
            );
        }
        JsResultQueryCondition::FinishTimeFrom(start) => {
            select = select.filter(js_result::Column::FinishTime.gte(*start));
        }
        JsResultQueryCondition::FinishTimeTo(end) => {
            select = select.filter(js_result::Column::FinishTime.lte(*end));
        }
        JsResultQueryCondition::IsSuccess => {
            select = select.filter(
                js_result::Column::Result
                    .is_not_null()
                    .and(js_result::Column::ErrorMessage.is_null()),
            );
        }
        JsResultQueryCondition::IsFailure => {
            select = select.filter(js_result::Column::ErrorMessage.is_not_null());
        }
        JsResultQueryCondition::IsRunning => {
            select = select.filter(
                js_result::Column::Result
                    .is_null()
                    .and(js_result::Column::ErrorMessage.is_null()),
            );
        }
        JsResultQueryCondition::Limit(_) | JsResultQueryCondition::Last => {}
    }

    select
}

pub async fn query(token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "js_result", condition_count = query.condition.len(), "processing js_result query request");
        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

        let mut select = js_result::Entity::find();
        let mut is_last = false;
        let mut limit_count: Option<u64> = None;
        let mut requested_worker_names: Vec<String> = query
            .condition
            .iter()
            .filter_map(|condition| {
                if let JsResultQueryCondition::JsWorkerName(name) = condition {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        requested_worker_names.sort();
        requested_worker_names.dedup();

        if requested_worker_names.is_empty() {
            let allowed_workers =
                resolve_accessible_js_result_workers(&token, JsResultAction::Read).await?;
            if allowed_workers.is_empty() {
                let json_str = "[]".to_owned();
                return RawValue::from_string(json_str)
                    .map_err(|e| NodegetError::SerializationError(e.to_string()).into());
            }
            select = select.filter(js_result::Column::JsWorkerName.is_in(allowed_workers));
        } else {
            for worker_name in &requested_worker_names {
                ensure_js_result_permission(&token, worker_name, JsResultAction::Read).await?;
            }
        }

        debug!(target: "js_result", condition_count = query.condition.len(), workers_count = requested_worker_names.len(), "js_result query permission check passed");

        for condition in &query.condition {
            match condition {
                JsResultQueryCondition::Limit(limit) => {
                    const MAX_LIMIT: u64 = 10_000;
                    limit_count = Some(std::cmp::min(*limit, MAX_LIMIT));
                }
                JsResultQueryCondition::Last => {
                    is_last = true;
                }
                _ => {
                    select = apply_filter_to_select(select, condition);
                }
            }
        }

        const DEFAULT_LIMIT: u64 = 1000;

        if is_last {
            select = select
                .order_by_desc(js_result::Column::StartTime)
                .order_by_desc(js_result::Column::Id)
                .limit(1);
        } else if let Some(limit) = limit_count {
            select = select
                .order_by_desc(js_result::Column::StartTime)
                .order_by_desc(js_result::Column::Id)
                .limit(limit);
        } else {
            select = select
                .order_by_desc(js_result::Column::StartTime)
                .order_by_desc(js_result::Column::Id)
                .limit(DEFAULT_LIMIT);
        }

        let results = select
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to query js_result: {e}")))?;

        debug!(target: "js_result", result_count = results.len(), "js_result query completed");

        let json_str = serde_json::to_string(&results).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize results: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
