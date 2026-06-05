//! `js-worker_delete` RPC —— 删除 JS Worker。
//!
//! 删除数据库记录后驱逐运行时池中的 Worker，确保下次不会使用旧实例。

use crate::js_worker::auth::check_js_worker_permission;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::JsWorker as JsWorkerPermission;
use ng_db::entity::js_worker;
use ng_db::get_db;
use ng_js_runtime::runtime_pool;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::{debug, trace};

/// 删除指定的 JS Worker。
///
/// - `token` —— 认证 Token
/// - `name` —— Worker 名称
///
/// 内部步骤：
/// 1. 校验 name 非空
/// 2. 检查 Delete 权限
/// 3. 从数据库删除 `js_worker` 记录
/// 4. 驱逐运行时池中的对应 Worker
pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }
        debug!(target: "js_worker", name = %name, "processing js_worker delete request");

        check_js_worker_permission(&token, &name, JsWorkerPermission::Delete).await?;

        debug!(target: "js_worker", name = %name, "js_worker delete permission check passed");

        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;
        let delete_result = js_worker::Entity::delete_many()
            .filter(js_worker::Column::Name.eq(&name))
            .exec(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        if delete_result.rows_affected == 0 {
            return Err(NodegetError::NotFound(format!("js_worker not found: {name}")).into());
        }
        runtime_pool::global_pool().evict_worker(&name);
        trace!(target: "js_worker", name = %name, "evicted worker from runtime pool after delete");

        debug!(target: "js_worker", name = %name, rows_affected = delete_result.rows_affected, "js_worker deleted successfully");

        let response = serde_json::json!({
            "success": true,
            "rows_affected": delete_result.rows_affected
        });
        serde_json::value::to_raw_value(&response)
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
