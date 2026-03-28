use crate::entity::js_worker;
use crate::js_runtime::compile_js_module_to_bytecode;
use crate::rpc::RpcHelper;
use crate::rpc::js_worker::JsWorkerRpcImpl;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::Utc;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use serde_json::value::RawValue;

pub async fn create(
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
        let existing = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        if existing.is_some() {
            return Err(NodegetError::InvalidInput(format!("js_worker already exists: {name}")).into());
        }

        let js_byte_code = tokio::task::spawn_blocking({
            let compile_input = js_script.clone();
            move || compile_js_module_to_bytecode(compile_input)
        })
        .await
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile task join failed: {e}")))?
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile failed: {e}")))?;

        let now_ms = Utc::now().timestamp_millis();
        let new_model = js_worker::ActiveModel {
            id: ActiveValue::NotSet,
            name: Set(name.clone()),
            js_script: Set(js_script),
            js_byte_code: Set(Some(js_byte_code)),
            env: Set(env),
            runtime_clean_time: Set(runtime_clean_time),
            create_at: Set(now_ms),
            update_at: Set(now_ms),
        };

        let inserted = new_model
            .insert(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let response = serde_json::json!({
            "id": inserted.id,
            "name": inserted.name,
            "create_at": inserted.create_at,
            "update_at": inserted.update_at
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
