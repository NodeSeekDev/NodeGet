use super::{JS_RT_MEMORY_LIMIT_BYTES, enrich_exception, init_js_runtime_globals, js_error};
use nodeget_lib::js_runtime::{RunType, RuntimePoolInfo, RuntimePoolWorkerInfo};
use nodeget_lib::utils::get_local_timestamp_ms_i64;
use rquickjs::{AsyncContext, AsyncRuntime, Error, Module, Promise, Value as JsValue};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::oneshot;
use tracing::{debug, info, trace, warn};

const RUNTIME_CLEAN_TIME_NONE: i64 = -1;
const CLEANUP_INTERVAL_MS: u64 = 1_000;

struct RuntimeState {
    rt: AsyncRuntime,
    ctx: AsyncContext,
    loaded_bytecode_hash: Option<u64>,
}

enum WorkerCommand {
    Execute {
        bytecode: Vec<u8>,
        bytecode_hash: u64,
        run_type: RunType,
        params: Value,
        env: Value,
        response_tx: oneshot::Sender<Result<Value, String>>,
    },
    Shutdown,
}

#[derive(Debug)]
struct RuntimeWorkerHandle {
    script_name: String,
    sender: std::sync::mpsc::Sender<WorkerCommand>,
    active_requests: AtomicUsize,
    last_used_ms: AtomicI64,
    runtime_clean_time_ms: AtomicI64,
}

impl RuntimeWorkerHandle {
    fn set_runtime_clean_time(&self, runtime_clean_time: Option<i64>) {
        let value = runtime_clean_time.unwrap_or(RUNTIME_CLEAN_TIME_NONE);
        self.runtime_clean_time_ms.store(value, Ordering::Relaxed);
    }

    fn runtime_clean_time(&self) -> Option<i64> {
        let value = self.runtime_clean_time_ms.load(Ordering::Relaxed);
        if value < 0 { None } else { Some(value) }
    }

    async fn execute(
        &self,
        bytecode: Vec<u8>,
        run_type: RunType,
        params: Value,
        env: Value,
    ) -> anyhow::Result<Value> {
        trace!(target: "js_runtime", "sending execute command to worker");
        self.active_requests.fetch_add(1, Ordering::SeqCst);

        let send_result = (|| {
            let bytecode_hash = hash_bytes(&bytecode);
            let (response_tx, response_rx) = oneshot::channel();
            let cmd = WorkerCommand::Execute {
                bytecode,
                bytecode_hash,
                run_type,
                params,
                env,
                response_tx,
            };

            self.sender
                .send(cmd)
                .map_err(|_| anyhow::anyhow!("Runtime worker channel closed"))?;

            Ok(response_rx)
        })();

        let response = match send_result {
            Ok(response_rx) => response_rx
                .await
                .map_err(|e| anyhow::anyhow!("Runtime worker dropped response: {e}")),
            Err(e) => Err(e),
        };

        match get_local_timestamp_ms_i64() {
            Ok(now) => self.last_used_ms.store(now, Ordering::Relaxed),
            Err(e) => {
                warn!(target: "js_runtime", error = %e, "Failed to read local timestamp for runtime worker");
            }
        }
        self.active_requests.fetch_sub(1, Ordering::SeqCst);

        match response? {
            Ok(value) => Ok(value),
            Err(message) => Err(anyhow::anyhow!(message)),
        }
    }
}

#[derive(Default)]
pub struct JsRuntimePool {
    workers: RwLock<HashMap<String, Arc<RuntimeWorkerHandle>>>,
}

impl JsRuntimePool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
        }
    }

    /// # Errors
    /// Returns an error if the worker channel is closed or script execution fails.
    pub async fn execute_script(
        &self,
        script_name: &str,
        bytecode: Vec<u8>,
        run_type: RunType,
        params: Value,
        env: Value,
        runtime_clean_time_ms: Option<i64>,
    ) -> anyhow::Result<Value> {
        debug!(target: "js_runtime", script_name = %script_name, run_type = ?run_type, "executing script on pool");
        let worker = self.get_or_init_worker(script_name)?;
        worker.set_runtime_clean_time(runtime_clean_time_ms);
        worker.execute(bytecode, run_type, params, env).await
    }

    #[allow(clippy::significant_drop_tightening)]
    fn get_or_init_worker(&self, script_name: &str) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
        debug!(target: "js_runtime", script_name = %script_name, "getting or initializing worker");
        {
            let workers = self.workers.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(worker) = workers.get(script_name).cloned() {
                return Ok(worker);
            }
        }

        let worker = spawn_worker(script_name)?;

        {
            let workers = self.workers.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(existing) = workers.get(script_name).cloned() {
                return Ok(existing);
            }
        }

        let mut workers = match self.workers.write() {
            Ok(guard) => guard,
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        };

        if let Some(existing) = workers.get(script_name).cloned() {
            return Ok(existing);
        }

        workers.insert(script_name.to_owned(), Arc::clone(&worker));
        Ok(worker)
    }

    pub fn cleanup_idle_workers(&self) {
        let now = get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp during runtime cleanup");
            0
        });

        let candidates: Vec<String> = match self.workers.read() {
            Ok(workers) => workers
                .iter()
                .filter_map(|(name, worker)| {
                    let clean_ms = worker.runtime_clean_time()?;

                    if clean_ms <= 0 {
                        return None;
                    }

                    if worker.active_requests.load(Ordering::SeqCst) > 0 {
                        return None;
                    }

                    if Arc::strong_count(worker) > 1 {
                        return None;
                    }

                    let last_used = worker.last_used_ms.load(Ordering::Relaxed);
                    if now.saturating_sub(last_used) >= clean_ms {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            Err(e) => {
                warn!(target: "js_runtime", error = %e, "Runtime pool read lock poisoned during cleanup");
                return;
            }
        };

        if candidates.is_empty() {
            return;
        }

        let mut workers = match self.workers.write() {
            Ok(guard) => guard,
            Err(e) => {
                warn!(target: "js_runtime", error = %e, "Runtime pool write lock poisoned during cleanup");
                return;
            }
        };

        for name in candidates {
            let should_remove = workers.get(&name).is_some_and(|worker| {
                let Some(clean_ms) = worker.runtime_clean_time() else {
                    return false;
                };

                if clean_ms <= 0 {
                    return false;
                }

                if worker.active_requests.load(Ordering::SeqCst) > 0 {
                    return false;
                }

                if Arc::strong_count(worker) > 1 {
                    return false;
                }

                let last_used = worker.last_used_ms.load(Ordering::Relaxed);
                now.saturating_sub(last_used) >= clean_ms
            });

            if !should_remove {
                continue;
            }

            if let Some(worker) = workers.remove(&name) {
                debug!(target: "js_runtime", worker_name = %name, "Cleaning idle JS runtime worker");
                let _ = worker.sender.send(WorkerCommand::Shutdown);
            }
        }
    }

    pub fn evict_worker(&self, script_name: &str) -> bool {
        let removed = match self.workers.write() {
            Ok(mut workers) => workers.remove(script_name),
            Err(e) => {
                warn!(target: "js_runtime", error = %e, "Runtime pool write lock poisoned during evict");
                return false;
            }
        };

        removed.is_some_and(|worker| {
            debug!(target: "js_runtime", worker_name = %script_name, "Evicting JS runtime worker");
            let _ = worker.sender.send(WorkerCommand::Shutdown);
            true
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimePoolInfo {
        let now = get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp during runtime snapshot");
            0
        });
        let workers = self
            .workers
            .read()
            .map(|guard| {
                guard
                    .values()
                    .map(|worker| {
                        let last_used = worker.last_used_ms.load(Ordering::Relaxed);
                        RuntimePoolWorkerInfo {
                            script_name: worker.script_name.clone(),
                            active_requests: worker.active_requests.load(Ordering::SeqCst),
                            last_used_ms: last_used,
                            idle_ms: now.saturating_sub(last_used),
                            runtime_clean_time_ms: worker.runtime_clean_time(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        RuntimePoolInfo {
            total_workers: workers.len(),
            workers,
        }
    }
}

static GLOBAL_RUNTIME_POOL: OnceLock<Arc<JsRuntimePool>> = OnceLock::new();
static CLEANUP_LOOP_STARTED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn global_pool() -> &'static Arc<JsRuntimePool> {
    GLOBAL_RUNTIME_POOL.get_or_init(|| Arc::new(JsRuntimePool::new()))
}

pub fn init_global_pool() -> &'static Arc<JsRuntimePool> {
    info!(target: "js_runtime", "initializing global JS runtime pool");
    let pool = global_pool();

    if !CLEANUP_LOOP_STARTED.swap(true, Ordering::SeqCst) {
        let pool_for_task = Arc::clone(pool);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(CLEANUP_INTERVAL_MS)).await;
                pool_for_task.cleanup_idle_workers();
            }
        });
    }

    pool
}

fn spawn_worker(script_name: &str) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
    debug!(target: "js_runtime", script_name = %script_name, "spawning new worker thread");
    let script_name = script_name.to_owned();
    let (tx, rx) = std::sync::mpsc::channel::<WorkerCommand>();

    let handle = Arc::new(RuntimeWorkerHandle {
        script_name: script_name.clone(),
        sender: tx,
        active_requests: AtomicUsize::new(0),
        last_used_ms: AtomicI64::new(get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp when spawning runtime worker");
            0
        })),
        runtime_clean_time_ms: AtomicI64::new(RUNTIME_CLEAN_TIME_NONE),
    });

    std::thread::Builder::new()
        .name(format!("js-rt-{script_name}"))
        .spawn(move || worker_loop(&script_name, rx))
        .map_err(|e| anyhow::anyhow!("Failed to spawn JS runtime worker thread: {e}"))?;

    Ok(handle)
}

fn worker_loop(script_name: &str, receiver: std::sync::mpsc::Receiver<WorkerCommand>) {
    trace!(target: "js_runtime", script_name = %script_name, "worker loop started");
    let host_rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            for cmd in receiver {
                if let WorkerCommand::Execute { response_tx, .. } = cmd {
                    let _ = response_tx.send(Err(format!(
                        "Failed to create runtime host for JS worker: {e}"
                    )));
                }
            }
            return;
        }
    };

    let mut runtime_state: Option<RuntimeState> = None;

    for cmd in receiver {
        match cmd {
            WorkerCommand::Execute {
                bytecode,
                bytecode_hash,
                run_type,
                params,
                env,
                response_tx,
            } => {
                let exec_result = host_rt.block_on(async {
                    execute_on_worker(
                        &mut runtime_state,
                        script_name,
                        bytecode,
                        bytecode_hash,
                        run_type,
                        params,
                        env,
                    )
                    .await
                    .map_err(|e| super::format_js_error(&e))
                });
                let _ = response_tx.send(exec_result);
            }
            WorkerCommand::Shutdown => break,
        }
    }
}

#[allow(clippy::future_not_send)]
async fn execute_on_worker(
    runtime_state: &mut Option<RuntimeState>,
    script_name: &str,
    bytecode: Vec<u8>,
    bytecode_hash: u64,
    run_type: RunType,
    params: Value,
    env: Value,
) -> Result<Value, Error> {
    trace!(target: "js_runtime", script_name = %script_name, "executing on worker");
    if runtime_state.is_none() {
        *runtime_state = Some(create_runtime_state().await?);
    }

    let state = runtime_state
        .as_mut()
        .ok_or_else(|| js_error("js_runtime", "Runtime state is missing"))?;

    if state.loaded_bytecode_hash != Some(bytecode_hash) {
        let load_result: Result<(), Error> = state
            .ctx
            .async_with(async |ctx| {
                let declared_module = enrich_exception(&ctx, "js_load", unsafe {
                    Module::load(ctx.clone(), &bytecode)
                })?;

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
                ctx.globals().set("__nodeget_entry", entry_value)?;

                Ok(())
            })
            .await;

        state.rt.idle().await;
        load_result?;
        state.loaded_bytecode_hash = Some(bytecode_hash);
    }

    let run_result: Result<Value, Error> = state
        .ctx
        .async_with(async |ctx| {
            let run_type_handler = run_type.handler_name().to_owned();
            ctx.globals()
                .set("__nodeget_run_handler", run_type_handler)?;

            let input_json = serde_json::to_string(&params).map_err(|e| {
                js_error(
                    "js_runner",
                    format!("Failed to serialize input params: {e}"),
                )
            })?;
            let input_js = ctx.json_parse(input_json).map_err(|e| {
                js_error(
                    "js_runner",
                    format!("Failed to build input params in JS: {e}"),
                )
            })?;
            ctx.globals().set("__nodeget_run_params", input_js)?;

            let env_json = serde_json::to_string(&env)
                .map_err(|e| js_error("js_runner", format!("Failed to serialize env: {e}")))?;
            let env_js = ctx
                .json_parse(env_json)
                .map_err(|e| js_error("js_runner", format!("Failed to build env in JS: {e}")))?;
            ctx.globals().set("__nodeget_env", env_js)?;

            ctx.globals()
                .set("__nodeget_current_script_name", script_name.to_owned())?;
            let inline_caller_js = ctx.json_parse("null").map_err(|e| {
                js_error(
                    "js_runner",
                    format!("Failed to set inline caller in JS: {e}"),
                )
            })?;
            ctx.globals()
                .set("__nodeget_inline_caller", inline_caller_js)?;

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
                    throw new Error(`Missing handler function export default.${runHandler}`);
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
                    const routeResponse = await handler.call(entry, routeRequest, env, runtimeCtx);

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

    state.rt.idle().await;
    run_result
}

#[allow(clippy::future_not_send)]
async fn create_runtime_state() -> Result<RuntimeState, Error> {
    trace!(target: "js_runtime", "creating new runtime state");
    let rt = AsyncRuntime::new()?;
    rt.set_memory_limit(JS_RT_MEMORY_LIMIT_BYTES).await;
    let ctx = AsyncContext::full(&rt).await?;

    let init_result: Result<(), Error> = ctx
        .async_with(async |ctx| init_js_runtime_globals(&ctx))
        .await;

    rt.idle().await;
    init_result?;

    Ok(RuntimeState {
        rt,
        ctx,
        loaded_bytecode_hash: None,
    })
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}
