use crate::entity::js_worker;
use crate::js_runtime::runtime_pool;
use crate::rpc::RpcHelper;
use crate::rpc::js_worker::JsWorkerRpcImpl;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;

pub async fn delete(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // TODO: token auth
        let _ = token;

        if name.trim().is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }

        let db = JsWorkerRpcImpl::get_db()?;
        let delete_result = js_worker::Entity::delete_many()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .exec(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        if delete_result.rows_affected == 0 {
            return Err(NodegetError::NotFound(format!("js_worker not found: {name}")).into());
        }
        runtime_pool::global_pool().evict_worker(name.as_str());

        let response = serde_json::json!({
            "success": true,
            "rows_affected": delete_result.rows_affected
        });
        let json_str = serde_json::to_string(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
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
