use crate::entity::js_worker;
use crate::rpc::RpcHelper;
use crate::rpc::js_worker::auth::check_js_worker_permission;
use crate::rpc::js_worker::JsWorkerRpcImpl;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::data_structure::JsWorker as JsWorkerPermission;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;

pub async fn read(token: String, name: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        if name.trim().is_empty() {
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }

        check_js_worker_permission(&token, name.as_str(), JsWorkerPermission::Read).await?;

        let db = JsWorkerRpcImpl::get_db()?;
        let model = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {name}")))?;
        let js_script_base64 = BASE64_STANDARD.encode(model.js_script.as_bytes());

        let response = serde_json::json!({
            "name": model.name,
            "route_name": model.route_name,
            "js_script_base64": js_script_base64,
            "runtime_clean_time": model.runtime_clean_time,
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
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
