use crate::js_worker::auth::check_js_worker_permission;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::JsWorker as JsWorkerPermission;
use ng_db::entity::js_worker;
use ng_db::get_db;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn read(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        if name.trim().is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }
        debug!(target: "js_worker", name = %name, "processing js_worker read request");

        check_js_worker_permission(&token, name.as_str(), JsWorkerPermission::Read).await?;

        debug!(target: "js_worker", name = %name, "js_worker read permission check passed");

        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;
        let model = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {name}")))?;
        let js_script_base64 = BASE64_STANDARD.encode(model.js_script.as_bytes());

        debug!(target: "js_worker", name = %model.name, "js_worker read completed");

        let response = serde_json::json!({
            "name": model.name,
            "description": model.description,
            "route_name": model.route_name,
            "js_script_base64": js_script_base64,
            "runtime_clean_time": model.runtime_clean_time,
            "max_run_time": model.max_run_time,
            "max_stack_size": model.max_stack_size,
            "max_heap_size": model.max_heap_size,
            "env": model.env,
            "create_at": model.create_at,
            "update_at": model.update_at
        });

        let json_str = serde_json::to_string(&response)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
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
