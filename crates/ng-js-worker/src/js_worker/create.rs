//! `js-worker_create` RPC —— 创建新的 JS Worker。
//!
//! 验证权限、解码脚本、编译字节码、检查唯一性后入库。

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
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::{debug, trace, warn};

/// 创建新的 JS Worker。
///
/// - `token` —— 认证 Token
/// - `name` —— Worker 名称（唯一标识）
/// - `description` —— 描述
/// - `js_script_base64` —— JS 脚本的 Base64 编码
/// - `route_name` —— HTTP 路由名称（可选）
/// - `runtime_clean_time` —— 空闲清理时间阈值（ms，可选）
/// - `env` —— 环境变量（可选）
/// - `max_run_time` —— 执行时长上限（ms，可选）
/// - `max_stack_size` —— 栈大小上限（bytes，可选）
/// - `max_heap_size` —— 堆大小上限（bytes，可选）
///
/// 内部步骤：
/// 1. 校验 name 非空、js_script_base64 非空且合法
/// 2. 检查 Create 权限
/// 3. 检查 name 和 route_name 唯一性
/// 4. 编译 JS 为字节码
/// 5. 插入 `js_worker` 表
#[allow(clippy::too_many_arguments)]
pub async fn create(
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
            warn!(target: "js_worker", "验证失败: name 为空");
            return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
        }
        debug!(target: "js_worker", name = %name, "processing js_worker create request");

        let route_name = normalize_route_name(route_name)?;

        check_js_worker_permission(&token, name.as_str(), JsWorkerPermission::Create).await?;
        debug!(target: "js_worker", name = %name, "js_worker create permission check passed");

        if js_script_base64.trim().is_empty() {
            warn!(target: "js_worker", name = %name, "验证失败: js_script_base64 为空");
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
            warn!(target: "js_worker", name = %name, "验证失败: 解码后 js_script 为空");
            return Err(
                NodegetError::InvalidInput("Decoded js_script cannot be empty".to_owned()).into(),
            );
        }

        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;
        let existing = js_worker::Entity::find()
            .filter(js_worker::Column::Name.eq(name.as_str()))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        if existing.is_some() {
            warn!(target: "js_worker", name = %name, "验证失败: js_worker 已存在");
            return Err(
                NodegetError::InvalidInput(format!("js_worker already exists: {name}")).into(),
            );
        }
        debug!(target: "js_worker", name = %name, "js_worker name available");

        if let Some(route_name) = route_name.as_deref() {
            let existing_route = js_worker::Entity::find()
                .filter(js_worker::Column::RouteName.eq(route_name))
                .one(db)
                .await
                .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;
            if existing_route.is_some() {
                warn!(target: "js_worker", name = %name, route_name = %route_name, "验证失败: route_name 已存在");
                return Err(NodegetError::InvalidInput(format!(
                    "route_name already exists: {route_name}"
                ))
                .into());
            }
        }

        trace!(target: "js_worker", name = %name, "submitting js module for bytecode compilation");
        let js_byte_code = tokio::task::spawn_blocking({
            let compile_input = js_script.clone();
            move || compile_js_module_to_bytecode(compile_input)
        })
        .await
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile task join failed: {e}")))?
        .map_err(|e| NodegetError::Other(format!("JavaScript precompile failed: {e}")))?;

        let now_ms = get_local_timestamp_ms_i64().unwrap_or(0);
        let new_model = js_worker::ActiveModel {
            id: ActiveValue::NotSet,
            name: Set(name.clone()),
            description: Set(description),
            js_script: Set(js_script),
            js_byte_code: Set(Some(js_byte_code)),
            route_name: Set(route_name.clone()),
            env: Set(env),
            runtime_clean_time: Set(runtime_clean_time),
            max_run_time: Set(max_run_time),
            max_stack_size: Set(max_stack_size),
            max_heap_size: Set(max_heap_size),
            create_at: Set(now_ms),
            update_at: Set(now_ms),
        };

        let inserted = new_model
            .insert(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        debug!(target: "js_worker", id = inserted.id, name = %inserted.name, "js_worker created successfully");

        let response = serde_json::json!({
            "id": inserted.id,
            "name": inserted.name,
            "description": inserted.description,
            "route_name": inserted.route_name,
            "runtime_clean_time": inserted.runtime_clean_time,
            "max_run_time": inserted.max_run_time,
            "max_stack_size": inserted.max_stack_size,
            "max_heap_size": inserted.max_heap_size,
            "create_at": inserted.create_at,
            "update_at": inserted.update_at
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
