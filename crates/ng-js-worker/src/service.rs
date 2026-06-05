//! JS Worker 执行服务 —— 入队执行、内联调用、结果记录。
//!
//! 三个核心入口：
//! - `enqueue_defined_js_worker_run` —— 字节码模式入队执行（使用运行时池）
//! - `enqueue_source_js_worker_run` —— 源码模式入队执行（使用一次性 Runtime）
//! - `run_inline_call_and_record_result` —— 内联调用执行（同步等待结果）
//!
//! 所有入口都遵循相同流程：查询 Worker 记录 → 插入 js_result 行 → 异步/同步执行 → 更新结果

use ng_core::error::NodegetError;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::{js_result, js_worker};
use ng_db::get_db;
use ng_js_runtime::{
    JsCodeInput, RunType, RuntimeLimits, compile_js_module_to_bytecode, format_js_error, js_runner,
    js_runner_source_mode, runtime_pool,
};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, error, info, trace};

/// 获取当前 QuickJS 字节码版本号（首字节）。
///
/// 在 `spawn_blocking` 中编译最小脚本提取 BC_VERSION，避免在 tokio runtime 内
/// 调用 `compile_js_module_to_bytecode`（其内部 `block_on` 会创建嵌套 runtime 导致 panic）。
/// 结果缓存在 `OnceLock` 中，后续调用零开销。
async fn current_bc_version() -> u8 {
    static VERSION: OnceLock<u8> = OnceLock::new();
    if let Some(&v) = VERSION.get() {
        return v;
    }
    let v = tokio::task::spawn_blocking(|| {
        compile_js_module_to_bytecode("0")
            .ok()
            .and_then(|bc| bc.first().copied())
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0);
    let _ = VERSION.set(v);
    v
}

/// 检查字节码版本是否匹配当前 QuickJS，不匹配时从源码重编译并更新 DB。
///
/// - 匹配：原样返回 `bytecode`
/// - 不匹配：用 `js_script` 重编译，写入 DB，驱逐运行时池中旧 worker，返回新字节码
pub async fn ensure_bytecode_version(
    model: &js_worker::Model,
    db: &sea_orm::DatabaseConnection,
) -> anyhow::Result<Vec<u8>> {
    let bytecode = model.js_byte_code.clone().ok_or_else(|| {
        NodegetError::InvalidInput(format!(
            "js_worker '{}' has no precompiled bytecode",
            model.name
        ))
    })?;

    let version = current_bc_version().await;
    if bytecode.first() == Some(&version) {
        return Ok(bytecode);
    }

    info!(
        target: "js_worker",
        name = %model.name,
        stored_version = bytecode.first().unwrap_or(&0),
        current_version = version,
        "Bytecode version mismatch, recompiling from source"
    );

    let new_bytecode = tokio::task::spawn_blocking({
        let source = model.js_script.clone();
        move || compile_js_module_to_bytecode(source)
    })
    .await
    .map_err(|e| NodegetError::Other(format!("Recompile task join failed: {e}")))?
    .map_err(|e| NodegetError::Other(format!("Bytecode recompile failed: {e}")))?;

    let mut active: js_worker::ActiveModel = model.clone().into();
    active.js_byte_code = Set(Some(new_bytecode.clone()));
    active
        .update(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    runtime_pool::global_pool().evict_worker(&model.name);

    Ok(new_bytecode)
}

/// 入队执行已编译的 JS Worker（字节码模式）。
///
/// 使用运行时池中的持久化 Worker 执行脚本，字节码缓存避免重复加载。
///
/// - `js_script_name` —— Worker 名称
/// - `run_type` —— 运行模式（Call/Cron/Route/InlineCall）
/// - `params` —— 调用参数
/// - `env_override` —— 环境变量覆盖，None 则使用数据库中的值
///
/// 内部步骤：
/// 1. 查询 `js_worker` 表获取字节码、限制配置等
/// 2. 插入 `js_result` 行（记录开始时间、参数等）
/// 3. 通过 `tokio::spawn` 异步执行脚本
/// 4. 执行完成后更新 `js_result` 行（记录结果或错误）
///
/// # Returns
/// 返回 `js_result` 行的 ID。
pub async fn enqueue_defined_js_worker_run(
    js_script_name: String,
    run_type: RunType,
    params: Value,
    env_override: Option<Value>,
) -> anyhow::Result<i64> {
    let script_name = js_script_name.trim().to_owned();
    if script_name.is_empty() {
        return Err(NodegetError::InvalidInput("js_script_name cannot be empty".to_owned()).into());
    }
    debug!(target: "js_worker", script_name = %script_name, run_type = ?run_type, "enqueuing defined js_worker run (bytecode)");

    let db = get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?
        .clone();
    let model = js_worker::Entity::find()
        .filter(js_worker::Column::Name.eq(script_name.as_str()))
        .one(&db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
        .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {script_name}")))?;

    let worker_id = model.id;
    let worker_name = model.name.clone();
    let bytecode = ensure_bytecode_version(&model, &db).await?;
    let runtime_clean_time = model.runtime_clean_time;
    let limits = RuntimeLimits::from_model(
        model.max_run_time,
        model.max_stack_size,
        model.max_heap_size,
    );
    let resolved_env =
        env_override.unwrap_or_else(|| model.env.unwrap_or_else(|| serde_json::json!({})));
    let run_type_text = run_type.as_str().to_owned();

    let start_time = get_local_timestamp_ms_i64().unwrap_or(0);
    let insert_result = js_result::Entity::insert(js_result::ActiveModel {
        id: ActiveValue::NotSet,
        js_worker_id: Set(worker_id),
        js_worker_name: Set(worker_name.clone()),
        run_type: Set(run_type_text),
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
    trace!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, "spawning bytecode execution task");

    tokio::spawn(async move {
        let run_outcome = runtime_pool::global_pool()
            .execute_script(
                worker_name.as_str(),
                bytecode,
                run_type,
                params,
                resolved_env,
                runtime_clean_time,
                limits,
            )
            .await;

        let finish_time = get_local_timestamp_ms_i64().unwrap_or(start_time);
        let duration_ms = finish_time - start_time;
        let (result_json, mut error_message): (Option<Value>, Option<String>) = match run_outcome {
            Ok(value) => {
                debug!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, duration_ms = duration_ms, "JS execution completed successfully");
                (Some(value), None)
            }
            Err(e) => {
                error!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, duration_ms = duration_ms, error = %e, "JS execution failed");
                (
                    None,
                    Some(format!("JavaScript runtime execution failed: {e}")),
                )
            }
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
            error!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, error = %e, "Failed to update js_result");
        }
    });

    Ok(js_result_id)
}

/// 执行内联调用并记录结果，同步等待执行完成。
///
/// 从另一个 JS Worker 内部调用目标 Worker，使用一次性 Runtime（`js_runner`）执行。
///
/// - `js_script_name` —— 目标 Worker 名称
/// - `params_json` —— 调用参数的 JSON 字符串（直接透传，避免冗余 parse/serialize）
/// - `timeout_sec` —— 调用方指定的软超时（秒），None 则不限
/// - `inline_caller` —— 发起调用的源 Worker 名称
///
/// 内部步骤：
/// 1. 解析 timeout_sec 为 Duration
/// 2. 解析 `params_json` 为 `Value`（用于 DB 记录）
/// 3. 查询 `js_worker` 表获取字节码、限制配置等
/// 4. 插入 `js_result` 行
/// 5. 通过 `spawn_blocking` 在阻塞线程池中执行 `js_runner`
/// 6. 等待执行完成，更新 `js_result` 行
/// 7. 返回执行结果的 JSON 字符串（直接透传，避免冗余 parse/serialize）
pub async fn run_inline_call_and_record_result(
    js_script_name: String,
    params_json: String,
    timeout_sec: Option<f64>,
    inline_caller: Option<String>,
) -> anyhow::Result<String> {
    let script_name = js_script_name.trim().to_owned();
    if script_name.is_empty() {
        return Err(NodegetError::InvalidInput("js_worker_name cannot be empty".to_owned()).into());
    }
    debug!(target: "js_worker", script_name = %script_name, timeout_sec = ?timeout_sec, inline_caller = ?inline_caller, "running inline call and recording result");

    // 解析 params_json 为 Value，仅用于 DB 记录；JS 执行层直接用字符串透传
    let params: Value = serde_json::from_str(&params_json).map_err(|e| {
        NodegetError::InvalidInput(format!("inline_call params is not valid JSON: {e}"))
    })?;

    let timeout_duration = match timeout_sec {
        Some(value) if value.is_finite() && value > 0.0 => Some(Duration::from_secs_f64(value)),
        Some(value) => {
            return Err(NodegetError::InvalidInput(format!(
                "timeout_sec must be a positive finite number, got: {value}"
            ))
            .into());
        }
        None => None,
    };

    let db = get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?
        .clone();
    let model = js_worker::Entity::find()
        .filter(js_worker::Column::Name.eq(script_name.as_str()))
        .one(&db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
        .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {script_name}")))?;

    let worker_id = model.id;
    let worker_name = model.name.clone();
    let bytecode = ensure_bytecode_version(&model, &db).await?;
    let limits = RuntimeLimits::from_model(
        model.max_run_time,
        model.max_stack_size,
        model.max_heap_size,
    );
    let env = model.env.unwrap_or_else(|| serde_json::json!({}));

    let start_time = get_local_timestamp_ms_i64().unwrap_or(0);
    let insert_result = js_result::Entity::insert(js_result::ActiveModel {
        id: ActiveValue::NotSet,
        js_worker_id: Set(worker_id),
        js_worker_name: Set(worker_name.clone()),
        run_type: Set(RunType::InlineCall.as_str().to_owned()),
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

    let target_script_name = worker_name.clone();
    let run_task = tokio::task::spawn_blocking(move || {
        js_runner(
            JsCodeInput::Bytecode(bytecode),
            RunType::InlineCall,
            params,
            env,
            Some(target_script_name),
            inline_caller,
            timeout_duration,
            limits,
        )
        .map_err(|e| {
            NodegetError::Other(format!(
                "JavaScript runtime execution failed: {}",
                format_js_error(&e)
            ))
            .into()
        })
    });

    let run_outcome: anyhow::Result<Value> = run_task.await.map_err(|e| {
        anyhow::Error::from(NodegetError::Other(format!(
            "inline_call task join failed: {e}"
        )))
    })?;

    let finish_time = get_local_timestamp_ms_i64().unwrap_or(start_time);
    let duration_ms = finish_time - start_time;
    let (result_json_for_db, mut error_message, return_str): (
        Option<Value>,
        Option<String>,
        anyhow::Result<String>,
    ) = match run_outcome {
        Ok(value) => {
            debug!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, duration_ms = duration_ms, "Inline call execution completed successfully");
            // 将 Value 转为 JSON 字符串直接返回，避免调用方再 parse→serialize 往返
            let json_str = serde_json::to_string(&value)
                .map_err(|e| anyhow::anyhow!("Failed to serialize inline_call result: {e}"))?;
            (Some(value), None, Ok(json_str))
        }
        Err(e) => {
            error!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, duration_ms = duration_ms, error = %e, "Inline call execution failed");
            (None, Some(e.to_string()), Err(e))
        }
    };

    if result_json_for_db.is_none() && error_message.is_none() {
        error_message = Some("JavaScript inline_call finished without result or error".to_owned());
    }

    if let Err(e) = js_result::Entity::update_many()
        .set(js_result::ActiveModel {
            finish_time: Set(Some(finish_time)),
            result: Set(result_json_for_db),
            error_message: Set(error_message),
            ..Default::default()
        })
        .filter(js_result::Column::Id.eq(js_result_id))
        .exec(&db)
        .await
    {
        error!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name, error = %e, "Failed to update js_result for inline_call");
    }

    return_str
}

/// 入队执行源码模式的 JS Worker。
///
/// 使用一次性 Runtime（`js_runner_source_mode`）执行脚本，每次重新解析编译。
///
/// - `js_script_name` —— Worker 名称
/// - `run_type` —— 运行模式
/// - `params` —— 调用参数
/// - `env_override` —— 环境变量覆盖，None 则使用数据库中的值
///
/// 内部步骤：
/// 1. 查询 `js_worker` 表获取源码、限制配置等
/// 2. 插入 `js_result` 行
/// 3. 通过 `tokio::spawn` + `spawn_blocking` 异步执行
/// 4. 执行完成后更新 `js_result` 行
///
/// # Returns
/// 返回 `js_result` 行的 ID。
pub async fn enqueue_source_js_worker_run(
    js_script_name: String,
    run_type: RunType,
    params: Value,
    env_override: Option<Value>,
) -> anyhow::Result<i64> {
    let script_name = js_script_name.trim().to_owned();
    if script_name.is_empty() {
        return Err(NodegetError::InvalidInput("js_script_name cannot be empty".to_owned()).into());
    }
    debug!(target: "js_worker", script_name = %script_name, run_type = ?run_type, "enqueuing source mode js_worker run");

    let db = get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?
        .clone();
    let model = js_worker::Entity::find()
        .filter(js_worker::Column::Name.eq(script_name.as_str()))
        .one(&db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?
        .ok_or_else(|| NodegetError::NotFound(format!("js_worker not found: {script_name}")))?;

    let worker_id = model.id;
    let worker_name = model.name.clone();
    let source_code = model.js_script;
    let limits = RuntimeLimits::from_model(
        model.max_run_time,
        model.max_stack_size,
        model.max_heap_size,
    );
    let resolved_env =
        env_override.unwrap_or_else(|| model.env.unwrap_or_else(|| serde_json::json!({})));
    let run_type_text = run_type.as_str().to_owned();

    let start_time = get_local_timestamp_ms_i64().unwrap_or(0);
    let insert_result = js_result::Entity::insert(js_result::ActiveModel {
        id: ActiveValue::NotSet,
        js_worker_id: Set(worker_id),
        js_worker_name: Set(worker_name.clone()),
        run_type: Set(run_type_text),
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

    let worker_name_for_log = worker_name.clone();
    trace!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name_for_log, "spawning source mode execution task");
    tokio::spawn(async move {
        let worker_name_in_spawn = worker_name_for_log.clone();
        let run_outcome: Result<Value, String> = match tokio::task::spawn_blocking(move || {
            js_runner_source_mode(
                &source_code,
                &worker_name,
                run_type,
                params,
                resolved_env,
                // source mode 没有调用方软超时；效果完全由 limits.max_run_time_ms 决定
                None,
                limits,
            )
        })
        .await
        {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(format!(
                "JavaScript execution error: {}",
                format_js_error(&e)
            )),
            Err(e) => Err(format!("Source mode task join failed: {e}")),
        };

        let finish_time = get_local_timestamp_ms_i64().unwrap_or(start_time);
        let duration_ms = finish_time - start_time;
        let (result_json, mut error_message): (Option<Value>, Option<String>) = match &run_outcome {
            Ok(value) => {
                debug!(target: "js_worker", js_result_id, worker = %worker_name_in_spawn, duration_ms, "Source mode JS execution completed successfully");
                (Some(value.clone()), None)
            }
            Err(e) => {
                error!(target: "js_worker", js_result_id, worker = %worker_name_in_spawn, duration_ms, error = %e, "Source mode JS execution failed");
                (
                    None,
                    Some(format!("JavaScript runtime execution failed: {e}")),
                )
            }
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
            error!(target: "js_worker", js_result_id = js_result_id, worker = %worker_name_in_spawn, error = %e, "Failed to update js_result for source mode worker");
        }
    });

    Ok(js_result_id)
}
