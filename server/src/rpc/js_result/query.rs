use crate::entity::js_result;
use crate::rpc::RpcHelper;
use crate::rpc::js_result::JsResultRpcImpl;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::js_result::query::{JsResultDataQuery, JsResultQueryCondition};
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;

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
        // TODO: token auth
        let _ = token;
        let db = JsResultRpcImpl::get_db()?;

        let mut select = js_result::Entity::find();
        let mut is_last = false;
        let mut limit_count: Option<u64> = None;

        for condition in &query.condition {
            match condition {
                JsResultQueryCondition::Limit(limit) => {
                    limit_count = Some(*limit);
                }
                JsResultQueryCondition::Last => {
                    is_last = true;
                }
                _ => {
                    select = apply_filter_to_select(select, condition);
                }
            }
        }

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
                .order_by_desc(js_result::Column::Id);
        }

        let results = select.all(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to query js_result: {e}"))
        })?;

        let json_str = serde_json::to_string(&results).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize results: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
