use nodeget_lib::js_runtime::{JsCodeInput, RunType};
use rquickjs::prelude::{Async, Func};
use rquickjs::{
    AsyncContext, AsyncRuntime, Ctx, Error, Module, Promise, Value as JsValue, WriteOptions,
};
use serde_json::Value;
use std::ffi::CString;
use uuid::Uuid;

pub mod inline_call;
pub mod nodeget;
pub mod runtime_pool;

pub(crate) const JS_RT_MEMORY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

pub fn js_error(stage: &'static str, message: impl Into<String>) -> Error {
    Error::new_from_js_message(stage, "String", message.into())
}

pub(crate) fn init_js_runtime_globals(ctx: &Ctx<'_>) -> Result<(), Error> {
    llrt_fetch::init(ctx)?;
    llrt_buffer::init(ctx)?;
    llrt_stream_web::init(ctx)?;
    llrt_url::init(ctx)?;
    llrt_util::init(ctx)?;
    llrt_timers::init(ctx)?;
    let global = ctx.globals();
    // Register raw Rust functions under internal names (return JSON strings)
    global.set("__nodeget_rpc_raw", Func::from(Async(nodeget::js_nodeget)))?;
    global.set(
        "__nodeget_inline_call_raw",
        Func::from(Async(inline_call::js_inline_call)),
    )?;
    global.set("uuid", Func::from(|| Uuid::new_v4().to_string()))?;
    // Wrap raw functions to return parsed JS objects instead of JSON strings
    ctx.eval::<(), _>(
        r#"
        globalThis.nodeget = async (json) => {
            const raw = await globalThis.__nodeget_rpc_raw(json);
            return JSON.parse(raw);
        };
        globalThis.__nodeget_inline_call = async (name, paramsJson, timeoutSec, caller) => {
            const raw = await globalThis.__nodeget_inline_call_raw(name, paramsJson, timeoutSec, caller);
            return JSON.parse(raw);
        };
        "#,
    )?;
    Ok(())
}

fn format_js_exception(ctx: &Ctx<'_>) -> String {
    let exception = ctx.catch();

    if let Some(obj) = exception.as_object() {
        let stack: Option<String> = obj.get("stack").ok();
        if let Some(stack) = stack
            && !stack.trim().is_empty()
        {
            return stack;
        }

        let name: Option<String> = obj.get("name").ok();
        let message: Option<String> = obj.get("message").ok();
        match (name, message) {
            (Some(name), Some(message)) if !message.is_empty() => {
                return format!("{name}: {message}");
            }
            (_, Some(message)) if !message.is_empty() => return message,
            _ => {}
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

fn enrich_exception<T>(
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

pub fn compile_js_module_to_bytecode(js_code: impl AsRef<str>) -> Result<Vec<u8>, Error> {
    let js_code = js_code.as_ref().to_owned();

    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| js_error("js_compile", format!("Failed to build host runtime: {e}")))?;

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        rt.set_memory_limit(JS_RT_MEMORY_LIMIT_BYTES).await;
        let ctx = AsyncContext::full(&rt).await?;

        let compile_result: Result<Vec<u8>, Error> = rquickjs::async_with!(ctx => |ctx| {
            // Keep compile context aligned with runtime context.
            init_js_runtime_globals(&ctx)?;

            compile_module_bytecode_no_eval(&ctx, &js_code)
        })
        .await;

        rt.idle().await;
        compile_result
    })
}

pub fn js_runner(
    js_code: JsCodeInput,
    run_type: RunType,
    input_params: Value,
    env_value: Value,
    current_script_name: Option<String>,
    inline_caller: Option<String>,
) -> Result<Value, Error> {
    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| js_error("js_runner", format!("Failed to build host runtime: {e}")))?;

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        rt.set_memory_limit(JS_RT_MEMORY_LIMIT_BYTES).await;
        let ctx = AsyncContext::full(&rt).await?;

        let js_result: Result<Value, Error> = rquickjs::async_with!(ctx => |ctx| {
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
                        nodeget: globalThis.nodeget,
                        uuid: globalThis.uuid,
                        runType: runHandler,
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
                        if (Array.isArray(input.body_bytes) && input.body_bytes.length > 0) {
                            routeInit.body = new Uint8Array(input.body_bytes);
                        }

                        const routeRequest = new Request(String(input.url ?? ""), routeInit);
                        const routeResponse = await handler(routeRequest, env, runtimeCtx);

                        if (!(routeResponse instanceof Response)) {
                            throw new Error("onRoute must return a Response object");
                        }

                        const routeBody = new Uint8Array(await routeResponse.arrayBuffer());
                        return {
                            status: routeResponse.status,
                            headers: Array.from(routeResponse.headers.entries())
                                .map(([name, value]) => ({ name, value })),
                            body_bytes: Array.from(routeBody)
                        };
                    }

                    const result = await handler(input, env, runtimeCtx);
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
    })
}

pub fn js_runner_source_mode(
    source_code: &str,
    script_name: &str,
    run_type: RunType,
    input_params: Value,
    env_value: Value,
) -> Result<Value, Error> {
    let host_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| js_error("js_runner", format!("Failed to build host runtime: {e}")))?;

    host_rt.block_on(async move {
        let rt = AsyncRuntime::new()?;
        rt.set_memory_limit(JS_RT_MEMORY_LIMIT_BYTES).await;
        let ctx = AsyncContext::full(&rt).await?;

        let js_result: Result<Value, Error> = rquickjs::async_with!(ctx => |ctx| {
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
            let module_name = format!("{}.js", script_name);
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
                        nodeget: globalThis.nodeget,
                        uuid: globalThis.uuid,
                        runType: runHandler,
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
                        if (Array.isArray(input.body_bytes) && input.body_bytes.length > 0) {
                            routeInit.body = new Uint8Array(input.body_bytes);
                        }

                        const routeRequest = new Request(String(input.url ?? ""), routeInit);
                        const routeResponse = await handler(routeRequest, env, runtimeCtx);

                        if (!(routeResponse instanceof Response)) {
                            throw new Error("onRoute must return a Response object");
                        }

                        const routeBody = new Uint8Array(await routeResponse.arrayBuffer());
                        return {
                            status: routeResponse.status,
                            headers: Array.from(routeResponse.headers.entries())
                                .map(([name, value]) => ({ name, value })),
                            body_bytes: Array.from(routeBody)
                        };
                    }

                    const result = await handler(input, env, runtimeCtx);
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
    })
}
