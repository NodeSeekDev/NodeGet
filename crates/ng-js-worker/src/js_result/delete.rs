//! `js-result` 删除操作 —— 按条件删除 JS 执行结果记录。
//!
//! 支持 ID、Worker 名称、运行类型、时间范围、状态等多种筛选条件，
//! 以及 Limit/Last 修饰符控制删除范围。

use crate::auth::{
    JsResultAction, ensure_js_result_permission, resolve_accessible_js_result_workers,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::js_result::query::JsResultDataQuery;
use ng_core::js_result::query::JsResultQueryCondition;
use ng_db::entity::js_result;
use ng_db::get_db;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

/// 删除符合条件的 JS 执行结果记录。
///
/// - `token` —— 认证 Token
/// - `query` —— 查询条件集合
///
/// 内部步骤：
/// 1. 解析条件中的 Worker 名称，确定权限范围
/// 2. 若未指定 Worker 名称，自动解析 token 可访问的所有 Worker
/// 3. 为 select 和 delete 两个查询分别应用筛选条件
/// 4. 若有 Limit/Last 修饰符，先 select ID 再按 ID 删除
/// 5. 否则直接 delete_many 按条件删除
pub async fn delete(token: String, query: JsResultDataQuery) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "js_result", condition_count = query.condition.len(), "processing js_result delete request");
        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

        let mut select_query = js_result::Entity::find()
            .select_only()
            .column(js_result::Column::Id);
        let mut delete_query = js_result::Entity::delete_many();

        let mut is_last = false;
        let mut limit_count: Option<u64> = None;
        let condition_count = query.condition.len();
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
                resolve_accessible_js_result_workers(&token, JsResultAction::Delete).await?;
            if allowed_workers.is_empty() {
                let response = serde_json::json!({
                    "success": true,
                    "deleted": 0,
                    "condition_count": condition_count,
                });
                return serde_json::value::to_raw_value(&response)
                    .map_err(|e| {
                        NodegetError::SerializationError(format!(
                            "Failed to serialize delete response: {e}"
                        ))
                    })
                    .map_err(|e| e.into());
            }
            select_query =
                select_query.filter(js_result::Column::JsWorkerName.is_in(allowed_workers.clone()));
            delete_query =
                delete_query.filter(js_result::Column::JsWorkerName.is_in(allowed_workers));
        } else {
            for worker_name in &requested_worker_names {
                ensure_js_result_permission(&token, worker_name, JsResultAction::Delete).await?;
            }
        }

        debug!(target: "js_result", condition_count, workers_count = requested_worker_names.len(), "js_result delete permission check passed");

        for condition in query.condition {
            match condition {
                JsResultQueryCondition::Id(id) => {
                    select_query = select_query.filter(js_result::Column::Id.eq(id));
                    delete_query = delete_query.filter(js_result::Column::Id.eq(id));
                }
                JsResultQueryCondition::JsWorkerId(js_worker_id) => {
                    select_query =
                        select_query.filter(js_result::Column::JsWorkerId.eq(js_worker_id));
                    delete_query =
                        delete_query.filter(js_result::Column::JsWorkerId.eq(js_worker_id));
                }
                JsResultQueryCondition::JsWorkerName(js_worker_name) => {
                    select_query = select_query
                        .filter(js_result::Column::JsWorkerName.eq(js_worker_name.clone()));
                    delete_query =
                        delete_query.filter(js_result::Column::JsWorkerName.eq(js_worker_name));
                }
                JsResultQueryCondition::RunType(run_type) => {
                    select_query =
                        select_query.filter(js_result::Column::RunType.eq(run_type.clone()));
                    delete_query = delete_query.filter(js_result::Column::RunType.eq(run_type));
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
                    select_query =
                        select_query.filter(js_result::Column::ErrorMessage.is_not_null());
                    delete_query =
                        delete_query.filter(js_result::Column::ErrorMessage.is_not_null());
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
                    // 单次删除上限 10000 条，防止误操作删除过多数据
                    const MAX_LIMIT: u64 = 10_000;
                    limit_count = Some(std::cmp::min(limit, MAX_LIMIT));
                }
                JsResultQueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        // Limit/Last 模式：先查 ID 再按 ID 删除，避免条件不精确导致误删
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
                    NodegetError::DatabaseError(format!(
                        "Failed to select js_result ids for delete: {e}"
                    ))
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
                .map_err(|e| {
                    NodegetError::DatabaseError(format!("Failed to delete js_result: {e}"))
                })?
                .rows_affected
        };

        let response = serde_json::json!({
            "success": true,
            "deleted": deleted_rows,
            "condition_count": condition_count,
        });

        debug!(target: "js_result", deleted_rows, condition_count, "js_result delete completed");
        serde_json::value::to_raw_value(&response)
            .map_err(|e| {
                NodegetError::SerializationError(format!(
                    "Failed to serialize delete response: {e}"
                ))
            })
            .map_err(|e| e.into())
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
