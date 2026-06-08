//! `agent.delete_dynamic` RPC 实现。
//!
//! 按 UUID、时间戳范围、入库时间范围等条件删除动态监控数据。
//! 支持 `Limit` 和 `Last` 条件：需要先查询 ID 再按 ID 删除，以兼容两种数据库后端。

use crate::query::QueryCondition;
use crate::rpc::agent::AgentRpcImpl;
use crate::rpc::agent::delete_common::{
    ResolvedCondition, extract_limit_and_last, resolve_conditions, scopes_from_conditions,
};
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{DynamicMonitoring, Permission};
use ng_core::permission::permission_checker::require_permission_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::dynamic_monitoring;
use ng_infra::server::RpcHelper;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::{debug, error, warn};

/// 删除动态监控数据。
///
/// - `token` — 身份认证凭据
/// - `conditions` — 查询条件列表
/// - 返回值 — `{"success": true, "deleted": N, "condition_count": M}`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Delete` 权限
/// 2. 解析条件中的 `Limit`/`Last` 标记和 `ResolvedCondition`
/// 3. 若有 Limit/Last：先查询 ID 列表，再按 ID 批量删除
/// 4. 否则：直接按条件构建 `delete_many` 并执行
#[allow(clippy::too_many_lines)]
pub async fn delete_dynamic(
    token: String,
    conditions: Vec<QueryCondition>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
        debug!(target: "monitoring", conditions_count = conditions.len(), "delete_dynamic: request received");

        let scopes = scopes_from_conditions(&conditions);
        let checker = require_permission_checker()?;
        let is_allowed = checker.check_token_limit(
            &token_or_auth,
            scopes,
            vec![Permission::DynamicMonitoring(DynamicMonitoring::Delete)],
        )
        .await?;

        if !is_allowed {
            warn!(target: "monitoring", "权限拒绝: 缺少 DynamicMonitoring Delete 权限");
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Missing DynamicMonitoring Delete permission for requested scope"
                    .to_owned(),
            )
                .into());
        }
        debug!(target: "monitoring", "delete_dynamic: permission check passed");

        let db = AgentRpcImpl::get_db()?;
        let (limit_count, is_last) = extract_limit_and_last(&conditions);
        let resolved_conditions = resolve_conditions(&conditions).await?;

        debug!(target: "monitoring", ?limit_count, is_last, "delete_dynamic: executing delete");

        let rows_affected = if is_last || limit_count.is_some() {
            let mut query = dynamic_monitoring::Entity::find();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(dynamic_monitoring::Column::UuidId.eq(*uuid_id));
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring::Column::Timestamp
                                .gte(*start)
                                .and(dynamic_monitoring::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(dynamic_monitoring::Column::Timestamp.gte(*start));
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(dynamic_monitoring::Column::Timestamp.lte(*end));
                    }
                    ResolvedCondition::StorageTimeFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring::Column::StorageTime
                                .gte(*start)
                                .and(dynamic_monitoring::Column::StorageTime.lte(*end)),
                        );
                    }
                    ResolvedCondition::StorageTimeFrom(start) => {
                        query = query.filter(dynamic_monitoring::Column::StorageTime.gte(*start));
                    }
                    ResolvedCondition::StorageTimeTo(end) => {
                        query = query.filter(dynamic_monitoring::Column::StorageTime.lte(*end));
                    }
                }
            }

            let limit = if is_last { 1 } else { limit_count.unwrap_or(0) };
            let ids: Vec<i64> = query
                .select_only()
                .column(dynamic_monitoring::Column::Id)
                .order_by_desc(dynamic_monitoring::Column::Timestamp)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| {
                    error!(target: "monitoring", error = %e, "Database query error");
                    NodegetError::DatabaseError(format!("Database query error: {e}"))
                })?;

            debug!(target: "monitoring", ids_count = ids.len(), limit, is_last, "Dynamic delete fetched IDs for limit/last path");

            if ids.is_empty() {
                0
            } else {
                dynamic_monitoring::Entity::delete_many()
                    .filter(dynamic_monitoring::Column::Id.is_in(ids))
                    .exec(db)
                    .await
                    .map_err(|e| {
                        error!(target: "monitoring", error = %e, "Database delete error");
                        NodegetError::DatabaseError(format!("Database delete error: {e}"))
                    })?
                    .rows_affected
            }
        } else {
            let mut query = dynamic_monitoring::Entity::delete_many();
            for cond in &resolved_conditions {
                match cond {
                    ResolvedCondition::UuidId(uuid_id) => {
                        query = query.filter(dynamic_monitoring::Column::UuidId.eq(*uuid_id));
                    }
                    ResolvedCondition::TimestampFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring::Column::Timestamp
                                .gte(*start)
                                .and(dynamic_monitoring::Column::Timestamp.lte(*end)),
                        );
                    }
                    ResolvedCondition::TimestampFrom(start) => {
                        query = query.filter(dynamic_monitoring::Column::Timestamp.gte(*start));
                    }
                    ResolvedCondition::TimestampTo(end) => {
                        query = query.filter(dynamic_monitoring::Column::Timestamp.lte(*end));
                    }
                    ResolvedCondition::StorageTimeFromTo(start, end) => {
                        query = query.filter(
                            dynamic_monitoring::Column::StorageTime
                                .gte(*start)
                                .and(dynamic_monitoring::Column::StorageTime.lte(*end)),
                        );
                    }
                    ResolvedCondition::StorageTimeFrom(start) => {
                        query = query.filter(dynamic_monitoring::Column::StorageTime.gte(*start));
                    }
                    ResolvedCondition::StorageTimeTo(end) => {
                        query = query.filter(dynamic_monitoring::Column::StorageTime.lte(*end));
                    }
                }
            }
            query
                .exec(db)
                .await
                .map_err(|e| {
                    error!(target: "monitoring", error = %e, "Database delete error");
                    NodegetError::DatabaseError(format!("Database delete error: {e}"))
                })?
                .rows_affected
        };

        debug!(target: "monitoring", rows_affected = rows_affected, conditions = conditions.len(), "Dynamic monitoring delete completed");

        serde_json::value::to_raw_value(&serde_json::json!({
            "success": true,
            "deleted": rows_affected,
            "condition_count": conditions.len()
        }))
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
