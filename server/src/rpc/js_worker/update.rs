use crate::entity::js_worker;
use crate::js_runtime::compile_js_module_to_bytecode;
use crate::js_runtime::runtime_pool;
use crate::rpc::RpcHelper;
use crate::rpc::js_worker::JsWorkerRpcImpl;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::Utc;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use serde_json::value::RawValue;

pub async fn update(
    token: String,
    name: String,
    js_script_base64: String,
    runtime_clean_time: Option<i64>,
    env: Option<Value>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // TODO: token auth
        let _ = token;

        if name.trim().is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }

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
            return Err(NodegetError::InvalidInput("Decoded js_script cannot be empty".to_owned()).into());
        }

        let db = JsWorkerRpcImpl::get_db()?;
        let model = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {name}")))?;

        let js_byte_code = tokio::task::spawn_blocking({
            let compile_input = js_script.clone();
            move || compile_js_module_to_bytecode(compile_input)
        })
        .await
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile task join failed: {e}")))?
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile failed: {e}")))?;

        let now_ms = Utc::now().timestamp_millis();
        let mut active_model: js_worker::ActiveModel = model.into();
        active_model.js_script = Set(js_script);
        active_model.js_byte_code = Set(Some(js_byte_code));
        active_model.runtime_clean_time = Set(runtime_clean_time);
        active_model.env = Set(env);
        active_model.update_at = Set(now_ms);

        let updated = active_model
            .update(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;
        runtime_pool::global_pool().evict_worker(updated.name.as_str());

        let response = serde_json::json!({
            "success": true,
            "name": updated.name,
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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
