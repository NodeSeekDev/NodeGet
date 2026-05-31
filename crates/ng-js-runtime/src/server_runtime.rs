use crate::{JsCodeInput, RunType};
use rquickjs::prelude::{Async, Func};
use rquickjs::{
    AsyncContext, AsyncRuntime, Ctx, Error, Module, Promise, Value as JsValue, WriteOptions,
};
use serde_json::Value;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, trace};
use uuid::Uuid;

pub(crate) const JS_RT_MEMORY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// `max_run_time` 的应用层默认值（ms）。见 `js_worker.max_run_time`，NULL 时兜底。
pub const DEFAULT_MAX_RUN_TIME_MS: u64 = 30_000;
/// `max_stack_size` 的应用层默认值（bytes）。QuickJS 本身默认 256 KiB，我们提到 1 MiB。
pub const DEFAULT_MAX_STACK_SIZE_BYTES: usize = 1024 * 1024;
/// `max_heap_size` 的应用层默认值（bytes）。与历史常量 `JS_RT_MEMORY_LIMIT_BYTES` 一致。
pub const DEFAULT_MAX_HEAP_SIZE_BYTES: usize = JS_RT_MEMORY_LIMIT_BYTES;

/// 来自 DB 的可选限制三元组，外加应用层默认兜底。
///
/// 三个字段对应 `js_worker` 表的 `max_run_time` / `max_stack_size` / `max_heap_size`。
/// 调用方拿到 `Option<i64>` 直接塞进 `RuntimeLimits::from_model`，后续所有 runtime
/// setup / 超时处理都走同一路径。
#[derive(Clone, Copy, Debug)]
pub struct RuntimeLimits {
    /// 执行时长硬上限（ms），外层 `tokio::time::timeout` + `set_interrupt_handler`。
    pub max_run_time_ms: u64,
    /// `QuickJS` C 栈字节数上限，`set_max_stack_size`。
    pub max_stack_size_bytes: usize,
    /// `QuickJS` 堆字节数上限，`set_memory_limit`。
    pub max_heap_size_bytes: usize,
}

impl RuntimeLimits {
    /// 从 `js_worker` 表三个可选字段构造；NULL / 非正数 / 超出 usize 范围 -> 兜底默认值。
    #[must_use]
    pub fn from_model(
        max_run_time_ms: Option<i64>,
        max_stack_size_bytes: Option<i64>,
        max_heap_size_bytes: Option<i64>,
    ) -> Self {
        fn pos_u64(v: Option<i64>, default: u64) -> u64 {
            match v {
                Some(n) if n > 0 => u64::try_from(n).unwrap_or(default),
                _ => default,
            }
        }
        fn pos_usize(v: Option<i64>, default: usize) -> usize {
            match v {
                Some(n) if n > 0 => usize::try_from(n).unwrap_or(default),
                _ => default,
            }
        }
        Self {
            max_run_time_ms: pos_u64(max_run_time_ms, DEFAULT_MAX_RUN_TIME_MS),
            max_stack_size_bytes: pos_usize(max_stack_size_bytes, DEFAULT_MAX_STACK_SIZE_BYTES),
            max_heap_size_bytes: pos_usize(max_heap_size_bytes, DEFAULT_MAX_HEAP_SIZE_BYTES),
        }
    }

    /// 纯默认，不读 DB 时的构造方式（compile、辅助路径等）。
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            max_run_time_ms: DEFAULT_MAX_RUN_TIME_MS,
            max_stack_size_bytes: DEFAULT_MAX_STACK_SIZE_BYTES,
            max_heap_size_bytes: DEFAULT_MAX_HEAP_SIZE_BYTES,
        }
    }

    /// 和调用方传入的软超时取 min，返回应该用于 `tokio::time::timeout` 的 Duration。
    ///
    /// `语义：调用方（inline_call` 的 `timeoutSec`）提出"我最多等这么久"，
    /// worker 配置的 `max_run_time_ms` 是"你最多跑这么久"——两者都要遵守，取较严的一个。
    #[must_use]
    pub fn effective_timeout(
        self,
        caller_soft_timeout: Option<std::time::Duration>,
    ) -> std::time::Duration {
        let hard = std::time::Duration::from_millis(self.max_run_time_ms);
        match caller_soft_timeout {
            Some(soft) => hard.min(soft),
            None => hard,
        }
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self::defaults()
    }
}

/// 对刚创建的 `AsyncRuntime` 应用 heap / stack 限制。
///
/// `set_memory_limit(0)` 在 `QuickJS` 里表示"无限"；我们始终传正数。
/// `set_max_stack_size` 必须在第一次执行脚本之前调用才有意义。
pub(crate) async fn apply_runtime_limits(rt: &AsyncRuntime, limits: RuntimeLimits) {
    rt.set_memory_limit(limits.max_heap_size_bytes).await;
    rt.set_max_stack_size(limits.max_stack_size_bytes).await;
}

/// 安装一个 interrupt handler，让 `set_kill_flag(true)` 能硬杀 JS 执行。
///
/// `QuickJS` 会在 JS 解释循环的检查点（function 调用、循环边、指令数等）
/// 回调 handler。返回 true 则 `QuickJS` 抛一个"不可捕获异常"，`try/catch`
/// 抓不住，脚本会被真正终止。我们把 flag 拿在外面：看门狗 OS 线程超时后
/// `store(true)`，`QuickJS` 下一个检查点就被打断。
///
/// 这样即便脚本里写的是 `while(true){}` 这种无 await 纯 CPU 循环，也能被杀。
pub(crate) async fn install_kill_handler(rt: &AsyncRuntime, kill_flag: Arc<AtomicBool>) {
    rt.set_interrupt_handler(Some(Box::new(move || kill_flag.load(Ordering::Relaxed))))
        .await;
}

/// 启动硬超时看门狗：独立 OS 线程，到时间仍未被 cancel 就 `store(true)`。
///
/// 关键点：rquickjs 的 `async_with` 在执行同步 JS（纯 CPU 循环）时会阻塞
/// 整个 tokio task，`tokio::time::timeout` 打不断，必须由一个**不在 tokio
/// 里**的看门狗来 set `kill_flag，让` `QuickJS` interrupt handler 在下个检查
/// 点读到 true 抛异常，才能真正硬杀 CPU 密集脚本。
///
/// 返回 `(cancel_tx, join_handle)`；执行成功结束时 drop `cancel_tx`（或
/// `send(())`）让看门狗线程立即退出，再 `join_handle.join()` 回收。
pub(crate) fn spawn_kill_watchdog(
    kill_flag: Arc<AtomicBool>,
    duration: std::time::Duration,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::Builder::new()
        .name("js-runner-watchdog".to_owned())
        .spawn(move || {
            // recv_timeout 返回 Err(Timeout) = 到点未被取消 -> 置 flag
            // 返回 Ok(_) 或 Err(Disconnected) = 被取消 / sender drop -> 正常退出
            if cancel_rx.recv_timeout(duration) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                kill_flag.store(true, Ordering::Relaxed);
            }
        })
        .expect("failed to spawn js-runner-watchdog OS thread");
    (cancel_tx, handle)
}

pub fn js_error(stage: &'static str, message: impl Into<String>) -> Error {
    Error::new_from_js_message(stage, "String", message.into())
}

/// Format a `rquickjs::Error` for human-readable display.
///
/// The default `Display` impl for `Error::FromJs` produces misleading output like
/// `"Error converting from js 'stage' into type 'String': actual message"`.
/// This function extracts the meaningful portion instead.
#[must_use]
pub fn format_js_error(err: &Error) -> String {
    match err {
        Error::FromJs {
            from,
            message: Some(msg),
            ..
        } if !msg.is_empty() => {
            format!("[{from}] {msg}")
        }
        other => other.to_string(),
    }
}

pub(crate) fn init_js_runtime_globals(ctx: &Ctx<'_>) -> Result<(), Error> {
    debug!(target: "js_runtime", "initializing JS runtime globals");
    llrt_fetch::init(ctx)?;
    llrt_buffer::init(ctx)?;
    llrt_stream_web::init(ctx)?;
    llrt_url::init(ctx)?;
    llrt_util::init(ctx)?;
    llrt_timers::init(ctx)?;
    let global = ctx.globals();
    // Register raw Rust functions under internal names (return JSON strings)
    global.set(
        "__nodeget_rpc_raw",
        Func::from(Async(crate::nodeget::js_nodeget)),
    )?;
    global.set(
        "__nodeget_inline_call_raw",
        Func::from(Async(crate::inline_call::js_inline_call)),
    )?;
    global.set("randomUUID", Func::from(|| Uuid::new_v4().to_string()))?;
    // Wrap raw functions to return parsed JS objects instead of JSON strings
    ctx.eval::<(), _>(
        r#"
        globalThis.nodeget = async (...args) => {
            let input;
            if (args.length <= 1) {
                const json = args[0];
                input = typeof json === 'string' ? json : JSON.stringify(json);
            } else {
                const method = args[0];
                const params = args[1];
                const id = args.length >= 3 ? args[2] : globalThis.randomUUID();
                input = JSON.stringify({ jsonrpc: "2.0", method, params, id });
            }
            const raw = await globalThis.__nodeget_rpc_raw(input);
            return JSON.parse(raw);
        };
        globalThis.__nodeget_inline_call = async (name, paramsJson, timeoutSec, caller) => {
            const raw = await globalThis.__nodeget_inline_call_raw(name, paramsJson, timeoutSec, caller);
            return JSON.parse(raw);
        };
        globalThis.execSql = async (token, sql, params) => {
            const resp = await nodeget("nodeget-server_exec_sql", {
                token: token,
                sql: sql,
                params: params !== undefined && params !== null ? params : null
            });
            if (resp.error) throw new Error(resp.error.message);
            return resp.result;
        };
        globalThis.getDatabaseType = async (token) => {
            const resp = await nodeget("nodeget-server_get_database_type", {
                token: token
            });
            if (resp.error) throw new Error(resp.error.message);
            return resp.result;
        };
        globalThis.db = {
            async create(token, name, opts) {
                const resp = await nodeget("db_create", { token, name, ...opts });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async read(token, name) {
                const resp = await nodeget("db_read", { token, name });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async update(token, name, newName) {
                const resp = await nodeget("db_update", { token, name, new_name: newName });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async remove(token, name) {
                const resp = await nodeget("db_delete", { token, name });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async list(token) {
                const resp = await nodeget("db_list", { token });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async execSql(token, name, sql, params) {
                const resp = await nodeget("db_exec_sql", {
                    token, name, sql,
                    params: params !== undefined && params !== null ? params : null
                });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
            async execTemplating(token, name, sql, params) {
                const resp = await nodeget("db_exec_templating", {
                    token, name, sql,
                    params: params !== undefined && params !== null ? params : null
                });
                if (resp.error) throw new Error(resp.error.message);
                return resp.result;
            },
        };
        "#,
    )?;
    Ok(())
}

fn format_js_exception(ctx: &Ctx<'_>) -> String {
    let exception = ctx.catch();

    if let Some(obj) = exception.as_object() {
        let name: Option<String> = obj.get("name").ok();
        let message: Option<String> = obj.get("message").ok();
        let stack: Option<String> = obj.get("stack").ok();

        // Build "Name: message" header when available
        let header = match (&name, &message) {
            (Some(name), Some(message)) if !message.is_empty() => {
                Some(format!("{name}: {message}"))
            }
            (_, Some(message)) if !message.is_empty() => Some(message.clone()),
            _ => None,
        };

        // QuickJS .stack only contains call frames without the error message,
        // so we must prepend the header to get a useful trace.
        if let Some(stack) = stack
            && !stack.trim().is_empty()
        {
            return if let Some(header) = header {
                format!("{header}\n{stack}")
            } else {
                stack
            };
        }

        if let Some(header) = header {
            return header;
        }
    }

    if let Ok(Some(json)) = ctx.json_stringify(exception.clone())
        && let Ok(raw) = json.to_string()
        && !raw.is_empty()
    {
        return raw;
    }

    format!("{exception:?}")
}

pub(crate) fn enrich_exception<T>(
    ctx: &Ctx<'_>,
    stage: &'static str,
    result: Result<T, Error>,
) -> Result<T, Error> {
    match result {
        Ok(value) => Ok(value),
        Err(err) if err.is_exception() => Err(js_error(stage, format_js_exception(ctx))),
        Err(err) => Err(err),
    }
}

fn compile_module_bytecode_no_eval(ctx: &Ctx<'_>, script: &str) -> Result<Vec<u8>, Error> {
    trace!(target: "js_runtime", "compiling module bytecode");
    let _ = CString::new(script.as_bytes())
        .map_err(|e| js_error("js_compile", format!("Script contains NUL byte: {e}")))?;
    let _ = CString::new("js_worker.js")
        .map_err(|e| js_error("js_compile", format!("Invalid filename: {e}")))?;

    let module = enrich_exception(
        ctx,
        "js_compile",
        Module::declare(ctx.clone(), "js_worker.js", script.as_bytes().to_vec()),
    )?;

    enrich_exception(ctx, "js_compile", module.write(WriteOptions::default()))
}

/// # Errors
/// Returns an error if the JS module cannot be compiled.
pub fn compile_js_module_to_bytecode(js_code: impl AsRef<str>) -> Result<Vec<u8>, Error> {
    debug!(target: "js_runtime", "compiling JS module to bytecode");
    let js_code = js_code.as_ref().to_owned();

    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| js_error("js_compile", format!("Failed to build host runtime: {e}")))?;

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        rt.set_memory_limit(JS_RT_MEMORY_LIMIT_BYTES).await;
        let ctx = AsyncContext::full(&rt).await?;

        let compile_result: Result<Vec<u8>, Error> = ctx
            .async_with(async |ctx| {
                // Keep compile context aligned with runtime context.
                init_js_runtime_globals(&ctx)?;

                compile_module_bytecode_no_eval(&ctx, &js_code)
            })
            .await;

        rt.idle().await;
        compile_result
    })
}

/// # Errors
/// Returns an error if building the host runtime or JS execution fails.
///
/// `limits` 来自 `js_worker` 表，控制 heap / stack / 执行时长硬上限。
/// `caller_soft_timeout` 是 `inline_call` / 调用方传入的软超时；与 `limits.max_run_time_ms`
/// 取较严的一个作为最终 `tokio::time::timeout` 时长。
pub fn js_runner(
    js_code: JsCodeInput,
    run_type: RunType,
    input_params: Value,
    env_value: Value,
    current_script_name: Option<String>,
    inline_caller: Option<String>,
    caller_soft_timeout: Option<std::time::Duration>,
    limits: RuntimeLimits,
) -> Result<Value, Error> {
    debug!(target: "js_runtime", run_type = ?run_type, has_inline_caller = inline_caller.is_some(), max_run_time_ms = limits.max_run_time_ms, max_stack_size_bytes = limits.max_stack_size_bytes, max_heap_size_bytes = limits.max_heap_size_bytes, "executing JS runner");
    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            error!(target: "js_runtime", error = %e, "failed to build host runtime in js_runner");
            js_error("js_runner", format!("Failed to build host runtime: {e}"))
        })?;

    let effective_timeout = limits.effective_timeout(caller_soft_timeout);

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        apply_runtime_limits(&rt, limits).await;
        // interrupt handler + 共享 kill_flag：外层 timeout 过后 store(true)，
        // QuickJS 下次检查点抛 uncatchable，纯 CPU while(true){} 也能打断。
        let kill_flag = Arc::new(AtomicBool::new(false));
        install_kill_handler(&rt, Arc::clone(&kill_flag)).await;

        let ctx = AsyncContext::full(&rt).await?;
        let execute = async {
            let js_result: Result<Value, Error> = ctx.async_with(async |ctx| {
                init_js_runtime_globals(&ctx)?;
                let global = ctx.globals();

                let run_type_handler = run_type.handler_name().to_owned();
                global.set("__nodeget_run_handler", run_type_handler)?;

                let input_json = serde_json::to_string(&input_params)
                    .map_err(|e| js_error("js_runner", format!("Failed to serialize input params: {e}")))?;
                let input_js = ctx
                    .json_parse(input_json)
                    .map_err(|e| js_error("js_runner", format!("Failed to build input params in JS: {e}")))?;
                global.set("__nodeget_run_params", input_js)?;

                let env_json = serde_json::to_string(&env_value)
                    .map_err(|e| js_error("js_runner", format!("Failed to serialize env: {e}")))?;
                let env_js = ctx.json_parse(env_json).map_err(|e| {
                    js_error(
                        "js_runner",
                        format!("Failed to build env object in JS: {e}"),
                    )
                })?;
                global.set("__nodeget_env", env_js)?;

                let current_script_name_json = serde_json::to_string(&current_script_name).map_err(|e| {
                    js_error(
                        "js_runner",
                        format!("Failed to serialize current script name: {e}"),
                    )
                })?;
                let current_script_name_js = ctx.json_parse(current_script_name_json).map_err(|e| {
                    js_error(
                        "js_runner",
                        format!("Failed to build current script name in JS: {e}"),
                    )
                })?;
                global.set("__nodeget_current_script_name", current_script_name_js)?;

                let inline_caller_json = serde_json::to_string(&inline_caller).map_err(|e| {
                    js_error(
                        "js_runner",
                        format!("Failed to serialize inline caller: {e}"),
                    )
                })?;
                let inline_caller_js = ctx.json_parse(inline_caller_json).map_err(|e| {
                    js_error("js_runner", format!("Failed to build inline caller in JS: {e}"))
                })?;
                global.set("__nodeget_inline_caller", inline_caller_js)?;

                let declared_module = match &js_code {
                    JsCodeInput::Source(source) => enrich_exception(
                        &ctx,
                        "js_load",
                        Module::declare(ctx.clone(), "js_worker.js", source.as_bytes().to_vec()),
                    )?,
                    JsCodeInput::Bytecode(bytecode) => enrich_exception(
                        &ctx,
                        "js_load",
                        unsafe { Module::load(ctx.clone(), bytecode) },
                    )?,
                };

                let (module, module_eval_promise) =
                    enrich_exception(&ctx, "js_eval", declared_module.eval())?;
                let _eval_result = enrich_exception(
                    &ctx,
                    "js_eval",
                    module_eval_promise.into_future::<JsValue<'_>>().await,
                )?;

                let namespace = enrich_exception(&ctx, "js_namespace", module.namespace())?;
                let entry_value: JsValue<'_> =
                    enrich_exception(&ctx, "js_namespace", namespace.get("default"))?;
                global.set("__nodeget_entry", entry_value)?;

                let invoke_script = r#"
                (async () => {
                    const entry = globalThis.__nodeget_entry;
                    const runHandler = globalThis.__nodeget_run_handler;
                    const input = globalThis.__nodeget_run_params;
                    const env = globalThis.__nodeget_env || {};
                    const inlineCall = async (jsWorkerName, callParams, timeoutSec = null) => {
                        const workerName = String(jsWorkerName ?? "").trim();
                        if (!workerName) {
                            throw new Error("inlineCall js_worker_name cannot be empty");
                        }

                        const timeoutValue =
                            timeoutSec === undefined || timeoutSec === null
                                ? null
                                : Number(timeoutSec);
                        if (
                            timeoutValue !== null &&
                            (!Number.isFinite(timeoutValue) || timeoutValue <= 0)
                        ) {
                            throw new Error(
                                "inlineCall timeout_sec must be a positive finite number"
                            );
                        }

                        let paramsJson = null;
                        try {
                            paramsJson = JSON.stringify(callParams);
                        } catch (e) {
                            throw new Error(
                                `inlineCall params is not JSON-serializable: ${e}`
                            );
                        }
                        if (typeof paramsJson !== "string") {
                            paramsJson = "null";
                        }

                        return await globalThis.__nodeget_inline_call(
                            workerName,
                            paramsJson,
                            timeoutValue,
                            globalThis.__nodeget_current_script_name ?? null
                        );
                    };
                    globalThis.inlineCall = inlineCall;
                    const runtimeCtx = {
                        runType: runHandler,
                        workerName: globalThis.__nodeget_current_script_name ?? null,
                        inlineCall,
                        inlineCaller: globalThis.__nodeget_inline_caller ?? null
                    };

                    if (!entry || typeof entry !== "object") {
                        throw new Error("export default must be an object");
                    }

                    const handler = entry[runHandler];

                    if (typeof handler !== "function") {
                        throw new Error(
                            `Missing handler function export default.${runHandler}`
                        );
                    }

                    if (runHandler === "onRoute") {
                        if (!input || typeof input !== "object") {
                            throw new Error("onRoute input must be an object");
                        }

                        const routeHeaders = Array.isArray(input.headers)
                            ? input.headers.map((h) => [
                                String(h?.name ?? ""),
                                String(h?.value ?? "")
                            ])
                            : [];
                        const routeInit = {
                            method: String(input.method ?? "GET"),
                            headers: routeHeaders
                        };
                        if (typeof input.body_base64 === 'string' && input.body_base64.length > 0) {
                            routeInit.body = Uint8Array.from(atob(input.body_base64), c => c.charCodeAt(0));
                        }

                        const routeRequest = new Request(String(input.url ?? ""), routeInit);
                        const routeResponse = await handler.call(entry, routeRequest, env, runtimeCtx);

                        if (!(routeResponse instanceof Response)) {
                            throw new Error("onRoute must return a Response object");
                        }

                        const routeBody = new Uint8Array(await routeResponse.arrayBuffer());
                        return {
                            status: routeResponse.status,
                            headers: Array.from(routeResponse.headers.entries())
                                .map(([name, value]) => ({ name, value })),
                            body_base64: Buffer.from(routeBody).toString('base64')
                        };
                    }

                    const result = await handler.call(entry, input, env, runtimeCtx);
                    if (typeof result === "undefined") {
                        throw new Error("JS handler must return a JSON-serializable value");
                    }

                    return result;
                })()
            "#;

                let invoke_promise: Promise<'_> =
                    enrich_exception(&ctx, "js_invoke", ctx.eval(invoke_script))?;
                let js_value: JsValue<'_> = enrich_exception(
                    &ctx,
                    "js_invoke",
                    invoke_promise.into_future::<JsValue<'_>>().await,
                )?;

                if js_value.is_undefined() {
                    return Err(js_error(
                        "json_parse",
                        "Script must return a JSON-serializable value",
                    ));
                }

                let raw_json = if let Some(js_string) = js_value.as_string() {
                    js_string.to_string()?
                } else {
                    let js_json_string = ctx.json_stringify(js_value)?.ok_or_else(|| {
                        js_error(
                            "json_parse",
                            "Script return is not JSON-serializable (got function/symbol)",
                        )
                    })?;
                    js_json_string.to_string()?
                };

                serde_json::from_str(&raw_json).map_err(|e| {
                    js_error(
                        "json_parse",
                        format!("Script return is not valid JSON: {e}"),
                    )
                })
            })
                .await;

            rt.idle().await;
            js_result
        };

        // 硬超时路径：OS 线程看门狗 + interrupt handler 打断 CPU 循环，
        // 外层 tokio::time::timeout 捕捉 async 路径上的挂起（await 点停在
        // 远端 I/O 等）。两层共同保障 max_run_time_ms 兜得住。
        let (cancel_tx, watchdog) = spawn_kill_watchdog(Arc::clone(&kill_flag), effective_timeout);
        let outcome = match tokio::time::timeout(effective_timeout, execute).await {
            Ok(result) => result,
            Err(_) => Err(js_error("js_runner", "JavaScript execution timed out")),
        };
        // 执行完/超时都 cancel 看门狗并回收线程
        let _ = cancel_tx.send(());
        let _ = watchdog.join();
        if kill_flag.load(Ordering::Relaxed) && outcome.is_err() {
            return Err(js_error(
                "js_runner",
                format!(
                    "JavaScript execution exceeded max_run_time_ms={}",
                    limits.max_run_time_ms
                ),
            ));
        }
        outcome
    })
}

/// # Errors
/// Returns an error if building the host runtime or JS execution fails.
///
/// 见 `js_runner` 的 `limits` 说明，语义一致。
pub fn js_runner_source_mode(
    source_code: &str,
    script_name: &str,
    run_type: RunType,
    input_params: Value,
    env_value: Value,
    caller_soft_timeout: Option<std::time::Duration>,
    limits: RuntimeLimits,
) -> Result<Value, Error> {
    debug!(target: "js_runtime", script_name = %script_name, run_type = ?run_type, max_run_time_ms = limits.max_run_time_ms, "executing JS runner in source mode");
    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| js_error("js_runner", format!("Failed to build host runtime: {e}")))?;

    let effective_timeout = limits.effective_timeout(caller_soft_timeout);

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        apply_runtime_limits(&rt, limits).await;
        let kill_flag = Arc::new(AtomicBool::new(false));
        install_kill_handler(&rt, Arc::clone(&kill_flag)).await;

        let ctx = AsyncContext::full(&rt).await?;
        let execute = async {
            let js_result: Result<Value, Error> = ctx.async_with(async |ctx| {
                init_js_runtime_globals(&ctx)?;
                let global = ctx.globals();

                let run_type_handler = run_type.handler_name().to_owned();
                global.set("__nodeget_run_handler", run_type_handler)?;

                let input_json = serde_json::to_string(&input_params)
                    .map_err(|e| js_error("js_runner", format!("Failed to serialize input params: {e}")))?;
                let input_js = ctx
                    .json_parse(input_json)
                    .map_err(|e| js_error("js_runner", format!("Failed to build input params in JS: {e}")))?;
                global.set("__nodeget_run_params", input_js)?;

                let env_json = serde_json::to_string(&env_value)
                    .map_err(|e| js_error("js_runner", format!("Failed to serialize env: {e}")))?;
                let env_js = ctx.json_parse(env_json).map_err(|e| {
                    js_error(
                        "js_runner",
                        format!("Failed to build env object in JS: {e}"),
                    )
                })?;
                global.set("__nodeget_env", env_js)?;

                global.set("__nodeget_current_script_name", script_name.to_owned())?;

                let inline_caller_js = ctx
                    .json_parse("null")
                    .map_err(|e| js_error("js_runner", format!("Failed to set inline caller in JS: {e}")))?;
                global.set("__nodeget_inline_caller", inline_caller_js)?;

                // Use actual script name for better error stack traces
                let module_name = format!("{script_name}.js");
                let declared_module = enrich_exception(
                    &ctx,
                    "js_load",
                    Module::declare(ctx.clone(), module_name, source_code.as_bytes().to_vec()),
                )?;

                let (module, module_eval_promise) =
                    enrich_exception(&ctx, "js_eval", declared_module.eval())?;
                let _eval_result = enrich_exception(
                    &ctx,
                    "js_eval",
                    module_eval_promise.into_future::<JsValue<'_>>().await,
                )?;

                let namespace = enrich_exception(&ctx, "js_namespace", module.namespace())?;
                let entry_value: JsValue<'_> =
                    enrich_exception(&ctx, "js_namespace", namespace.get("default"))?;
                global.set("__nodeget_entry", entry_value)?;

                let invoke_script = r#"
                (async () => {
                    const entry = globalThis.__nodeget_entry;
                    const runHandler = globalThis.__nodeget_run_handler;
                    const input = globalThis.__nodeget_run_params;
                    const env = globalThis.__nodeget_env || {};
                    const inlineCall = async (jsWorkerName, callParams, timeoutSec = null) => {
                        const workerName = String(jsWorkerName ?? "").trim();
                        if (!workerName) {
                            throw new Error("inlineCall js_worker_name cannot be empty");
                        }

                        const timeoutValue =
                            timeoutSec === undefined || timeoutSec === null
                                ? null
                                : Number(timeoutSec);
                        if (
                            timeoutValue !== null &&
                            (!Number.isFinite(timeoutValue) || timeoutValue <= 0)
                        ) {
                            throw new Error(
                                "inlineCall timeout_sec must be a positive finite number"
                            );
                        }

                        let paramsJson = null;
                        try {
                            paramsJson = JSON.stringify(callParams);
                        } catch (e) {
                            throw new Error(
                                `inlineCall params is not JSON-serializable: ${e}`
                            );
                        }
                        if (typeof paramsJson !== "string") {
                            paramsJson = "null";
                        }

                        return await globalThis.__nodeget_inline_call(
                            workerName,
                            paramsJson,
                            timeoutValue,
                            globalThis.__nodeget_current_script_name ?? null
                        );
                    };
                    globalThis.inlineCall = inlineCall;
                    const runtimeCtx = {
                        runType: runHandler,
                        workerName: globalThis.__nodeget_current_script_name ?? null,
                        inlineCall,
                        inlineCaller: globalThis.__nodeget_inline_caller ?? null
                    };

                    if (!entry || typeof entry !== "object") {
                        throw new Error("export default must be an object");
                    }

                    const handler = entry[runHandler];

                    if (typeof handler !== "function") {
                        throw new Error(
                            `Missing handler function export default.${runHandler}`
                        );
                    }

                    if (runHandler === "onRoute") {
                        if (!input || typeof input !== "object") {
                            throw new Error("onRoute input must be an object");
                        }

                        const routeHeaders = Array.isArray(input.headers)
                            ? input.headers.map((h) => [
                                String(h?.name ?? ""),
                                String(h?.value ?? "")
                            ])
                            : [];
                        const routeInit = {
                            method: String(input.method ?? "GET"),
                            headers: routeHeaders
                        };
                        if (typeof input.body_base64 === 'string' && input.body_base64.length > 0) {
                            routeInit.body = Uint8Array.from(atob(input.body_base64), c => c.charCodeAt(0));
                        }

                        const routeRequest = new Request(String(input.url ?? ""), routeInit);
                        const routeResponse = await handler.call(entry, routeRequest, env, runtimeCtx);

                        if (!(routeResponse instanceof Response)) {
                            throw new Error("onRoute must return a Response object");
                        }

                        const routeBody = new Uint8Array(await routeResponse.arrayBuffer());
                        return {
                            status: routeResponse.status,
                            headers: Array.from(routeResponse.headers.entries())
                                .map(([name, value]) => ({ name, value })),
                            body_base64: Buffer.from(routeBody).toString('base64')
                        };
                    }

                    const result = await handler.call(entry, input, env, runtimeCtx);
                    if (typeof result === "undefined") {
                        throw new Error("JS handler must return a JSON-serializable value");
                    }

                    return result;
                })()
            "#;

                let invoke_promise: Promise<'_> =
                    enrich_exception(&ctx, "js_invoke", ctx.eval(invoke_script))?;
                let js_value: JsValue<'_> = enrich_exception(
                    &ctx,
                    "js_invoke",
                    invoke_promise.into_future::<JsValue<'_>>().await,
                )?;

                if js_value.is_undefined() {
                    return Err(js_error(
                        "json_parse",
                        "Script must return a JSON-serializable value",
                    ));
                }

                let raw_json = if let Some(js_string) = js_value.as_string() {
                    js_string.to_string()?
                } else {
                    let js_json_string = ctx.json_stringify(js_value)?.ok_or_else(|| {
                        js_error(
                            "json_parse",
                            "Script return is not JSON-serializable (got function/symbol)",
                        )
                    })?;
                    js_json_string.to_string()?
                };

                serde_json::from_str(&raw_json).map_err(|e| {
                    js_error(
                        "json_parse",
                        format!("Script return is not valid JSON: {e}"),
                    )
                })
            })
                .await;

            rt.idle().await;
            js_result
        };

        let (cancel_tx, watchdog) = spawn_kill_watchdog(Arc::clone(&kill_flag), effective_timeout);
        let outcome = match tokio::time::timeout(effective_timeout, execute).await {
            Ok(result) => result,
            Err(_) => Err(js_error("js_runner", "JavaScript execution timed out")),
        };
        let _ = cancel_tx.send(());
        let _ = watchdog.join();
        if kill_flag.load(Ordering::Relaxed) && outcome.is_err() {
            return Err(js_error(
                "js_runner",
                format!(
                    "JavaScript execution exceeded max_run_time_ms={}",
                    limits.max_run_time_ms
                ),
            ));
        }
        outcome
    })
}
