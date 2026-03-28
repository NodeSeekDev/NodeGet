use crate::entity::js_result;
use crate::rpc::RpcHelper;
use crate::rpc::js_result::JsResultRpcImpl;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::js_result::query::JsResultDataQuery;
use nodeget_lib::js_result::query::JsResultQueryCondition;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;

pub async fn delete(token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // TODO: token auth
        let _ = token;
        let db = JsResultRpcImpl::get_db()?;

        let mut select_query = js_result::Entity::find()
            .select_only()
            .column(js_result::Column::Id);
        let mut delete_query = js_result::Entity::delete_many();

        let mut is_last = false;
        let mut limit_count: Option<u64> = None;
        let condition_count = query.condition.len();

        for condition in query.condition {
            match condition {
                JsResultQueryCondition::Id(id) => {
                    select_query = select_query.filter(js_result::Column::Id.eq(id));
                    delete_query = delete_query.filter(js_result::Column::Id.eq(id));
                }
                JsResultQueryCondition::JsWorkerId(js_worker_id) => {
                    select_query =
                        select_query.filter(js_result::Column::JsWorkerId.eq(js_worker_id));
                    delete_query = delete_query.filter(js_result::Column::JsWorkerId.eq(js_worker_id));
                }
                JsResultQueryCondition::JsWorkerName(js_worker_name) => {
                    select_query =
                        select_query.filter(js_result::Column::JsWorkerName.eq(js_worker_name.clone()));
                    delete_query = delete_query.filter(js_result::Column::JsWorkerName.eq(js_worker_name));
                }
                JsResultQueryCondition::StartTimeFromTo(start, end) => {
                    select_query = select_query.filter(
                        js_result::Column::StartTime
                            .gte(start)
                            .and(js_result::Column::StartTime.lte(end)),
                    );
                    delete_query = delete_query.filter(
                        js_result::Column::StartTime
                            .gte(start)
                            .and(js_result::Column::StartTime.lte(end)),
                    );
                }
                JsResultQueryCondition::StartTimeFrom(start) => {
                    select_query = select_query.filter(js_result::Column::StartTime.gte(start));
                    delete_query = delete_query.filter(js_result::Column::StartTime.gte(start));
                }
                JsResultQueryCondition::StartTimeTo(end) => {
                    select_query = select_query.filter(js_result::Column::StartTime.lte(end));
                    delete_query = delete_query.filter(js_result::Column::StartTime.lte(end));
                }
                JsResultQueryCondition::FinishTimeFromTo(start, end) => {
                    select_query = select_query.filter(
                        js_result::Column::FinishTime
                            .gte(start)
                            .and(js_result::Column::FinishTime.lte(end)),
                    );
                    delete_query = delete_query.filter(
                        js_result::Column::FinishTime
                            .gte(start)
                            .and(js_result::Column::FinishTime.lte(end)),
                    );
                }
                JsResultQueryCondition::FinishTimeFrom(start) => {
                    select_query = select_query.filter(js_result::Column::FinishTime.gte(start));
                    delete_query = delete_query.filter(js_result::Column::FinishTime.gte(start));
                }
                JsResultQueryCondition::FinishTimeTo(end) => {
                    select_query = select_query.filter(js_result::Column::FinishTime.lte(end));
                    delete_query = delete_query.filter(js_result::Column::FinishTime.lte(end));
                }
                JsResultQueryCondition::IsSuccess => {
                    select_query = select_query.filter(
                        js_result::Column::Result
                            .is_not_null()
                            .and(js_result::Column::ErrorMessage.is_null()),
                    );
                    delete_query = delete_query.filter(
                        js_result::Column::Result
                            .is_not_null()
                            .and(js_result::Column::ErrorMessage.is_null()),
                    );
                }
                JsResultQueryCondition::IsFailure => {
                    select_query = select_query.filter(js_result::Column::ErrorMessage.is_not_null());
                    delete_query = delete_query.filter(js_result::Column::ErrorMessage.is_not_null());
                }
                JsResultQueryCondition::IsRunning => {
                    select_query = select_query.filter(
                        js_result::Column::Result
                            .is_null()
                            .and(js_result::Column::ErrorMessage.is_null()),
                    );
                    delete_query = delete_query.filter(
                        js_result::Column::Result
                            .is_null()
                            .and(js_result::Column::ErrorMessage.is_null()),
                    );
                }
                JsResultQueryCondition::Limit(limit) => {
                    limit_count = Some(limit);
                }
                JsResultQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        let deleted_rows = if is_last || limit_count.is_some() {
            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = select_query
                .order_by_desc(js_result::Column::StartTime)
                .order_by_desc(js_result::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    NodegetError::DatabaseError(format!("Failed to select js_result ids for delete: {e}"))
                })?;

            if ids.is_empty() {
                0
            } else {
                js_result::Entity::delete_many()
                    .filter(js_result::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        NodegetError::DatabaseError(format!("Failed to delete js_result: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            delete_query
                .exec(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(format!("Failed to delete js_result: {e}")))?
                .rows_affected
        };

        let response = serde_json::json!({
            "success": true,
            "deleted": deleted_rows,
            "condition_count": condition_count,
        });
        let json_str = serde_json::to_string(&response).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize delete response: {e}"))
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
