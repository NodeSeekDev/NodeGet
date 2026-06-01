use crate::server_runtime::{
    INVOKE_SCRIPT_JS, RuntimeLimits, apply_runtime_limits, enrich_exception, format_js_error,
    init_js_runtime_globals, install_kill_handler, js_error, prepare_invoke_globals,
    resolve_invoke_result, spawn_kill_watchdog,
};
use crate::{RunType, RuntimePoolInfo, RuntimePoolWorkerInfo};
use ng_core::utils::get_local_timestamp_ms_i64;
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
    /// 本 worker 的 heap/stack 是在首次创建时固定的；记录下来以便后续 stats 或日志使用。
    limits: RuntimeLimits,
    /// interrupt handler 在 worker `创建时安装，kill_flag` 被共享；每次 execute 前
    /// `store(false)`，完成后一并处理。
    kill_flag: Arc<AtomicBool>,
}

enum WorkerCommand {
    Execute {
        bytecode: Vec<u8>,
        bytecode_hash: u64,
        run_type: RunType,
        params: Value,
        env: Value,
        /// 本次执行的 `max_run_time（ms）。heap/stack` 已在 worker 创建时固定，
        /// 这里只用于 per-call 硬超时看门狗。
        max_run_time_ms: u64,
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
        max_run_time_ms: u64,
    ) -> anyhow::Result<Value> {
        trace!(target: "js_runtime", "sending execute command to worker");
        self.active_requests.fetch_add(1, Ordering::SeqCst);
        let _guard = ActiveRequestGuard(&self.active_requests);

        let send_result = (|| {
            let bytecode_hash = hash_bytes(&bytecode);
            let (response_tx, response_rx) = oneshot::channel();
            let cmd = WorkerCommand::Execute {
                bytecode,
                bytecode_hash,
                run_type,
                params,
                env,
                max_run_time_ms,
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

        match response? {
            Ok(value) => Ok(value),
            Err(message) => Err(anyhow::anyhow!(message)),
        }
    }
}

struct ActiveRequestGuard<'a>(&'a AtomicUsize);

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
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
    ///
    /// `limits` 来自 `js_worker` 表的 `max_run_time` / `max_stack_size` / `max_heap_size`。
    /// heap/stack 在 worker 首次创建时固定，update.rs 调用 `evict_worker` 强制下次
    /// 重建时采用新值；`max_run_time_ms` 每次调用生效。
    pub async fn execute_script(
        &self,
        script_name: &str,
        bytecode: Vec<u8>,
        run_type: RunType,
        params: Value,
        env: Value,
        runtime_clean_time_ms: Option<i64>,
        limits: RuntimeLimits,
    ) -> anyhow::Result<Value> {
        debug!(target: "js_runtime", script_name = %script_name, run_type = ?run_type, max_run_time_ms = limits.max_run_time_ms, "executing script on pool");
        let worker = self.get_or_init_worker(script_name, limits)?;
        worker.set_runtime_clean_time(runtime_clean_time_ms);
        worker
            .execute(bytecode, run_type, params, env, limits.max_run_time_ms)
            .await
    }

    #[allow(clippy::significant_drop_tightening)]
    fn get_or_init_worker(
        &self,
        script_name: &str,
        limits: RuntimeLimits,
    ) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
        debug!(target: "js_runtime", script_name = %script_name, "getting or initializing worker");
        {
            let workers = self.workers.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(worker) = workers.get(script_name).cloned() {
                return Ok(worker);
            }
        }

        let worker = spawn_worker(script_name, limits)?;

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
    GLOBAL_RUNTIME_POOL.get_or_init(|| {
        info!(target: "js_runtime", "initializing global JS runtime pool");
        Arc::new(JsRuntimePool::new())
    })
}

pub fn init_global_pool() -> &'static Arc<JsRuntimePool> {
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

fn spawn_worker(
    script_name: &str,
    limits: RuntimeLimits,
) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
    debug!(target: "js_runtime", script_name = %script_name, max_run_time_ms = limits.max_run_time_ms, max_stack_size_bytes = limits.max_stack_size_bytes, max_heap_size_bytes = limits.max_heap_size_bytes, "spawning new worker thread");
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
        .spawn(move || worker_loop(&script_name, rx, limits))
        .map_err(|e| anyhow::anyhow!("Failed to spawn JS runtime worker thread: {e}"))?;

    Ok(handle)
}

fn worker_loop(
    script_name: &str,
    receiver: std::sync::mpsc::Receiver<WorkerCommand>,
    limits: RuntimeLimits,
) {
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
                max_run_time_ms,
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
                        limits,
                        max_run_time_ms,
                    )
                    .await
                    .map_err(|e| format_js_error(&e))
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
    limits: RuntimeLimits,
    max_run_time_ms: u64,
) -> Result<Value, Error> {
    trace!(target: "js_runtime", script_name = %script_name, "executing on worker");
    if runtime_state.is_none() {
        *runtime_state = Some(create_runtime_state(limits).await?);
    }

    let state = runtime_state
        .as_mut()
        .ok_or_else(|| js_error("js_runtime", "Runtime state is missing"))?;

    // 每次执行前清 flag（上一轮超时留下的 true 会立刻打断新执行）
    state.kill_flag.store(false, Ordering::Relaxed);

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

    // 以 OS 线程看门狗启动硬超时；async_with 里可能卡在纯 CPU 循环上，
    // tokio::time::timeout 打不断它，必须靠 interrupt handler 读 kill_flag。
    let effective_timeout = std::time::Duration::from_millis(max_run_time_ms);
    let (cancel_tx, watchdog) =
        spawn_kill_watchdog(Arc::clone(&state.kill_flag), effective_timeout);

    let run_future = state.ctx.async_with(async |ctx| {
        prepare_invoke_globals(
            &ctx,
            run_type.handler_name(),
            &params,
            &env,
            Some(script_name),
            None,
        )?;

        let invoke_promise: Promise<'_> =
            enrich_exception(&ctx, "js_invoke", ctx.eval(INVOKE_SCRIPT_JS))?;
        let js_value: JsValue<'_> = enrich_exception(
            &ctx,
            "js_invoke",
            invoke_promise.into_future::<JsValue<'_>>().await,
        )?;

        resolve_invoke_result(&ctx, js_value)
    });
    let run_outcome: Result<Value, Error> =
        match tokio::time::timeout(effective_timeout, run_future).await {
            Ok(result) => result,
            Err(_) => Err(js_error("js_runner", "JavaScript execution timed out")),
        };
    let _ = cancel_tx.send(());
    let _ = watchdog.join();

    // 判定是否是因为硬超时被 interrupt 打断。interrupt 会让 QuickJS 抛不可
    // 捕获异常，但 runtime 内部可能仍残留 pending jobs / 待清理的 promise
    // reactions。pool 场景下这个 AsyncRuntime 之后会继续服务新请求，残留
    // 状态可能影响下一次执行——最稳的做法是丢弃当前 state，下次调用走
    // `create_runtime_state` 重建一个干净的 runtime。
    let killed_by_timeout = state.kill_flag.load(Ordering::Relaxed) && run_outcome.is_err();

    // Reset kill flag so interrupt handler won't abort the timer-clear eval below.
    // killed status is already stored in `killed_by_timeout`.
    if killed_by_timeout {
        state.kill_flag.store(false, Ordering::Relaxed);
    }

    // Clear all timers to prevent idle() from hanging on persistent setInterval
    // and to clean RT_TIMER_STATE entries before potential runtime destruction.
    // kill_flag is now false, so the interrupt handler won't interfere.
    let _ = state.ctx.async_with(async |ctx| {
        let _ = ctx.eval::<(), _>(
            "if(typeof globalThis.__nodeget_clear_all_timers==='function')globalThis.__nodeget_clear_all_timers()"
        );
    }).await;

    if killed_by_timeout {
        // Drop 整个 runtime state：释放 AsyncContext + AsyncRuntime，
        // QuickJS 释放所有 JS 对象（含未消费 Incoming body 的 Response），
        // 向 hyper 连接 task 发出异步关闭信号。
        *runtime_state = None;

        // drop 后关闭信号已发出，需要 runtime 继续轮询才能处理。
        // 否则被中断的 fetch 连接将永远停留在 CLOSE_WAIT。
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        return Err(js_error(
            "js_runner",
            format!("JavaScript execution exceeded max_run_time_ms={max_run_time_ms}"),
        ));
    }

    // Bounded idle — safety net: if cleanup missed persistent async work,
    // 100ms timeout triggers and we discard the runtime state.
    let idle_ok = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        state.rt.idle()
    ).await.is_ok();

    if !idle_ok {
        // idle() timed out — runtime has unkillable persistent work, discard it
        *runtime_state = None;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        return Err(js_error("js_runner", "Runtime cleanup timed out — state discarded"));
    }

    // 给 tokio runtime 一个短窗口来处理挂起的 I/O 清理。
    //
    // JS 执行期间 fetch() 产生的 Response 对象可能未被完全消费（未调
    // .text()/.json() 等）。rt.idle() 期间 QuickJS GC 可能回收了部分
    // Response，其持有的 hyper Incoming body 被 drop，向连接 task 发出
    // 异步关闭信号。但 pool worker 的 current_thread runtime 在
    // block_on 返回后不再被轮询，关闭信号将无法处理——TCP 连接停留在
    // CLOSE_WAIT（远端已 FIN，本地未 FIN，Recv-Q 有残留字节）。
    //
    // 此 drain 让 runtime 继续运转一小段时间，使连接 task 能处理关闭
    // 信号、发送 FIN、释放 socket。10ms 相对 30s 默认超时可忽略。
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    run_outcome
}

#[allow(clippy::future_not_send)]
async fn create_runtime_state(limits: RuntimeLimits) -> Result<RuntimeState, Error> {
    trace!(target: "js_runtime", max_run_time_ms = limits.max_run_time_ms, max_stack_size_bytes = limits.max_stack_size_bytes, max_heap_size_bytes = limits.max_heap_size_bytes, "creating new runtime state");
    let rt = AsyncRuntime::new()?;
    apply_runtime_limits(&rt, limits).await;
    let kill_flag = Arc::new(AtomicBool::new(false));
    install_kill_handler(&rt, Arc::clone(&kill_flag)).await;
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
        limits,
        kill_flag,
    })
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}
