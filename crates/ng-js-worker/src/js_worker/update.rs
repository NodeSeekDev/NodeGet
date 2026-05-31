use crate::js_worker::auth::check_js_worker_permission;
use crate::js_worker::route_name::normalize_route_name;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::JsWorker as JsWorkerPermission;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::js_worker;
use ng_db::get_db;
use ng_js_runtime::compile_js_module_to_bytecode;
use ng_js_runtime::runtime_pool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, trace};

pub async fn update(
    token: String,
    name: String,
    description: Option<String>,
    js_script_base64: String,
    route_name: Option<String>,
    runtime_clean_time: Option<i64>,
    env: Option<Value>,
    max_run_time: Option<i64>,
    max_stack_size: Option<i64>,
    max_heap_size: Option<i64>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }
        debug!(target: "js_worker", name = %name, "processing js_worker update request");

        let route_name = normalize_route_name(route_name)?;

        check_js_worker_permission(&token, name.as_str(), JsWorkerPermission::Write).await?;

        debug!(target: "js_worker", name = %name, "js_worker update permission check passed");

        if js_script_base64.trim().is_empty() {
            return Err(
                NodegetError::InvalidInput("js_script_base64 cannot be empty".to_owned()).into(),
            );
        }

        let js_script_bytes = BASE64_STANDARD
            .decode(js_script_base64.as_bytes())
            .map_err(|e| NodegetError::ParseError(format!("Invalid js_script_base64: {e}")))?;
        let js_script = String::from_utf8(js_script_bytes).map_err(|e| {
            NodegetError::ParseError(format!("js_script_base64 is not valid UTF-8: {e}"))
        })?;

        if js_script.trim().is_empty() {
            return Err(
                NodegetError::InvalidInput("Decoded js_script cannot be empty".to_owned()).into(),
            );
        }

        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;
        let model = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {name}")))?;

        if let Some(route_name) = route_name.as_deref() {
            let existing_route = js_worker::Entity::find()
                .filter(js_worker::Column::RouteName.eq(route_name))
                .filter(js_worker::Column::Name.ne(name.as_str()))
                .one(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

            if existing_route.is_some() {
                return Err(NodegetError::InvalidInput(format!(
                    "route_name already exists: {route_name}"
                ))
                .into());
            }
        }

        trace!(target: "js_worker", name = %name, "submitting js module for bytecode recompilation");
        let js_byte_code = tokio::task::spawn_blocking({
            let compile_input = js_script.clone();
            move || compile_js_module_to_bytecode(compile_input)
        })
        .await
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile task join failed: {e}")))?
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile failed: {e}")))?;

        let now_ms = get_local_timestamp_ms_i64().unwrap_or(0);
        let mut active_model: js_worker::ActiveModel = model.into();
        active_model.js_script = Set(js_script);
        active_model.js_byte_code = Set(Some(js_byte_code));
        active_model.description = Set(description);
        active_model.route_name = Set(route_name);
        active_model.runtime_clean_time = Set(runtime_clean_time);
        active_model.max_run_time = Set(max_run_time);
        active_model.max_stack_size = Set(max_stack_size);
        active_model.max_heap_size = Set(max_heap_size);
        active_model.env = Set(env);
        active_model.update_at = Set(now_ms);

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;
        runtime_pool::global_pool().evict_worker(updated.name.as_str());
        trace!(target: "js_worker", name = %updated.name, "evicted worker from runtime pool after update");

        debug!(target: "js_worker", name = %updated.name, "js_worker updated successfully");

        let response = serde_json::json!({
            "success": true,
            "name": updated.name,
            "description": updated.description,
            "route_name": updated.route_name,
            "runtime_clean_time": updated.runtime_clean_time,
            "max_run_time": updated.max_run_time,
            "max_stack_size": updated.max_stack_size,
            "max_heap_size": updated.max_heap_size,
            "update_at": updated.update_at
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
