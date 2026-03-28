use crate::entity::{js_result, js_worker};
use crate::js_runtime::runtime_pool;
use crate::rpc::RpcHelper;
use crate::rpc::js_worker::JsWorkerRpcImpl;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::error::NodegetError;
use nodeget_lib::js_runtime::RunType;
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use serde_json::value::RawValue;

pub async fn run(
    token: String,
    js_script_name: String,
    run_type: Option<RunType>,
    params: Value,
    env: Option<Value>,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // TODO: token auth
        let _ = token;
        let run_type = run_type.unwrap_or(RunType::Call);

        let script_name = js_script_name.trim().to_owned();

        if script_name.is_empty() {
            return Err(NodegetError::InvalidInput("js_script_name cannot be empty".to_owned()).into());
        }

        let db = JsWorkerRpcImpl::get_db()?.clone();
        let model = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(script_name.as_str()))
            .one(&db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
            .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {script_name}")))?;

        let worker_id = model.id;
        let worker_name = model.name.clone();
        let bytecode = model.js_byte_code.ok_or_else(|| {
            NodegetError::InvalidInput(format!(
                "js_worker '{script_name}' has no precompiled bytecode"
            ))
        })?;
        let runtime_clean_time = model.runtime_clean_time;
        let resolved_env = env.unwrap_or_else(|| model.env.unwrap_or_else(|| serde_json::json!({})));

        let start_time = get_local_timestamp_ms_i64().unwrap_or(0);
        let insert_result = js_result::Entity::insert(js_result::ActiveModel {
            id: ActiveValue::NotSet,
            js_worker_id: Set(worker_id),
            js_worker_name: Set(worker_name.clone()),
            start_time: Set(Some(start_time)),
            finish_time: Set(None),
            param: Set(Some(params.clone())),
            result: Set(None),
            error_message: Set(None),
        })
        .exec(&db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let js_result_id = insert_result.last_insert_id;

        tokio::spawn(async move {
            let run_outcome = runtime_pool::init_global_pool()
                .execute_script(
                    worker_name.as_str(),
                    bytecode,
                    run_type,
                    params,
                    resolved_env,
                    runtime_clean_time,
                )
                .await;

            let finish_time = get_local_timestamp_ms_i64().unwrap_or(start_time);
            let (result_json, mut error_message): (Option<Value>, Option<String>) = match run_outcome
            {
                Ok(value) => (Some(value), None),
                Err(e) => (
                    None,
                    Some(format!("JavaScript runtime execution failed: {e}")),
                ),
            };

            if result_json.is_none() && error_message.is_none() {
                error_message = Some("JavaScript run finished without result or error".to_owned());
            }

            if let Err(e) = js_result::Entity::update_many()
                .set(js_result::ActiveModel {
                    finish_time: Set(Some(finish_time)),
                    result: Set(result_json),
                    error_message: Set(error_message),
                    ..Default::default()
                })
                .filter(js_result::Column::Id.eq(js_result_id))
                .exec(&db)
                .await
            {
                error!(
                    "Failed to update js_result {} for worker '{}': {}",
                    js_result_id, worker_name, e
                );
            }
        });

        let json_str = serde_json::to_string(&serde_json::json!({ "id": js_result_id }))
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
