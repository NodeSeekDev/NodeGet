//! `QuickJS` 运行时池 —— 每个注册脚本对应一个 OS 线程 + `QuickJS` 实例。
//!
//! 核心架构：
//! - 每个 `RuntimeWorkerHandle` 持有一个独立 OS 线程，内部运行 current-thread Tokio Runtime
//! - 通过 `std::sync::mpsc` channel 发送 `WorkerCommand`（Execute / Shutdown）
//! - 字节码缓存：`loaded_bytecode_hash` 避免相同字节码重复加载
//! - 空闲清理：`cleanup_idle_workers` 定期扫描超过 `runtime_clean_time` 且无活跃请求的 Worker
//! - 硬超时：`kill_flag` + interrupt handler 机制打断 CPU 密集循环
//!
//! 与 `server_runtime` 模块的区别：此模块维护持久化的 Worker 池，而 `js_runner`/`js_runner_source_mode`
//! 是一次性执行（用完即弃）。

use crate::server_runtime::{
    INVOKE_SCRIPT_JS, RuntimeLimits, apply_runtime_limits, enrich_exception, format_js_error,
    init_js_runtime_globals, install_kill_handler, js_error, prepare_invoke_globals,
    register_watchdog, resolve_invoke_result,
};
use crate::{RunType, RuntimePoolInfo, RuntimePoolWorkerInfo};
use ng_core::utils::get_local_timestamp_ms_i64;
use rquickjs::{AsyncContext, AsyncRuntime, Error, Module, Promise, Value as JsValue};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::oneshot;
use tracing::{debug, info, trace, warn};

/// `runtime_clean_time` 为 None 时的哨兵值。
const RUNTIME_CLEAN_TIME_NONE: i64 = -1;
/// 空闲清理扫描间隔（ms）。
const CLEANUP_INTERVAL_MS: u64 = 5_000;
/// I/O drain 窗口（ms）。Worker 的 current-thread runtime 在 `block_on` 返回后不再被轮询，
/// 此窗口让 hyper 连接 task 处理关闭信号，防止 TCP 停留在 `CLOSE_WAIT`。
const DRAIN_IO_MS: u64 = 100;

/// 单个 Worker 线程内的运行时状态，持有 `QuickJS` `AsyncRuntime` 和 `AsyncContext`。
struct RuntimeState {
    /// `QuickJS` `AsyncRuntime` 实例
    rt: AsyncRuntime,
    /// `QuickJS` `AsyncContext` 实例
    ctx: AsyncContext,
    /// 当前已加载字节码的哈希值，用于判断是否需要重新加载
    loaded_bytecode_hash: Option<u64>,
    /// 本 Worker 的 heap/stack 是在首次创建时固定的；记录下来以便后续 stats 或日志使用。
    #[allow(dead_code)]
    limits: RuntimeLimits,
    /// interrupt handler 在 Worker 创建时安装，`kill_flag` 被共享；每次 execute 前
    /// `store(false)`，完成后一并处理。
    kill_flag: Arc<AtomicBool>,
}

/// Worker 线程接收的命令枚举。
enum WorkerCommand {
    /// 执行脚本命令
    Execute {
        /// `QuickJS` 字节码，`None` 表示与上次相同（复用缓存），`Some` 表示新字节码
        bytecode: Option<Arc<Vec<u8>>>,
        /// 字节码哈希值，用于判断是否需要重新加载
        bytecode_hash: u64,
        /// 运行模式
        run_type: RunType,
        /// 调用参数（`Arc` 共享避免克隆开销）
        params: Arc<Value>,
        /// 环境变量（`Arc` 共享避免克隆开销）
        env: Arc<Value>,
        /// 本次执行的 `max_run_time`（ms）。heap/stack 已在 Worker 创建时固定，
        /// 这里只用于 per-call 硬超时看门狗。
        max_run_time_ms: u64,
        /// 执行结果的一次性发送通道
        response_tx: oneshot::Sender<Result<Value, String>>,
    },
    /// 关闭 Worker 线程
    Shutdown,
}

/// Worker 线程的句柄，由池持有以发送命令和查询状态。
#[derive(Debug)]
struct RuntimeWorkerHandle {
    /// Worker 对应的脚本名称
    script_name: String,
    /// 命令发送通道（有界，防止背压失控）
    sender: std::sync::mpsc::SyncSender<WorkerCommand>,
    /// 当前活跃请求数（原子计数）
    active_requests: AtomicUsize,
    /// 上次使用时间（ms，unix epoch）
    last_used_ms: AtomicI64,
    /// 运行时清理时间阈值（ms），负值表示永不清理
    runtime_clean_time_ms: AtomicI64,
    /// 上次发送的字节码哈希值，0 表示未发送过（用于判断是否可省略 bytecode 传输）
    last_bytecode_hash: AtomicU64,
}

impl RuntimeWorkerHandle {
    /// 设置运行时清理时间阈值。
    fn set_runtime_clean_time(&self, runtime_clean_time: Option<i64>) {
        let value = runtime_clean_time.unwrap_or(RUNTIME_CLEAN_TIME_NONE);
        self.runtime_clean_time_ms.store(value, Ordering::Relaxed);
    }

    /// 获取运行时清理时间阈值，None 表示永不清理。
    fn runtime_clean_time(&self) -> Option<i64> {
        let value = self.runtime_clean_time_ms.load(Ordering::Relaxed);
        if value < 0 { None } else { Some(value) }
    }

    /// 向 Worker 发送执行命令并等待结果。
    ///
    /// - `bytecode` —— `QuickJS` 字节码
    /// - `run_type` —— 运行模式
    /// - `params` —— 调用参数
    /// - `env` —— 环境变量
    /// - `max_run_time_ms` —— 本次硬超时（ms）
    ///
    /// 内部步骤：
    /// 1. 原子递增 `active_requests`，RAII guard 保证退出时递减
    /// 2. 计算字节码哈希，若与缓存相同则不发送字节码（Worker 端复用）
    /// 3. 构造 `WorkerCommand::Execute` 用 `try_send` 发送到 Worker 线程
    /// 4. 等待 oneshot 通道返回结果
    /// 5. 更新 `last_used_ms` 时间戳
    async fn execute(
        &self,
        bytecode: Vec<u8>,
        run_type: RunType,
        params: Value,
        env: Value,
        max_run_time_ms: u64,
    ) -> anyhow::Result<Value> {
        trace!(target: "js_runtime", "sending execute command to worker");
        self.active_requests.fetch_add(1, Ordering::AcqRel);
        let _guard = ActiveRequestGuard(&self.active_requests);

        let send_result = (|| {
            let bytecode_hash = hash_bytes(&bytecode);
            let (response_tx, response_rx) = oneshot::channel();

            // 若哈希与上次相同，发送 None 让 Worker 复用缓存字节码，省略传输
            let last_hash = self.last_bytecode_hash.load(Ordering::Acquire);
            let bytecode_opt = if last_hash == bytecode_hash {
                None
            } else {
                Some(Arc::new(bytecode))
            };

            let cmd = WorkerCommand::Execute {
                bytecode: bytecode_opt,
                bytecode_hash,
                run_type,
                params: Arc::new(params),
                env: Arc::new(env),
                max_run_time_ms,
                response_tx,
            };

            self.sender
                .try_send(cmd)
                .map_err(|_| anyhow::anyhow!("Worker queue full, request rejected"))?;

            // 发送成功后更新哈希缓存
            self.last_bytecode_hash
                .store(bytecode_hash, Ordering::Release);

            Ok(response_rx)
        })();

        let response = match send_result {
            Ok(response_rx) => response_rx
                .await
                .map_err(|e| anyhow::anyhow!("Runtime worker dropped response: {e}")),
            Err(e) => Err(e),
        };

        match get_local_timestamp_ms_i64() {
            Ok(now) => self.last_used_ms.store(now, Ordering::Release),
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

/// RAII guard，Drop 时自动递减 `active_requests` 计数。
struct ActiveRequestGuard<'a>(&'a AtomicUsize);

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// `QuickJS` 运行时池，管理所有持久化的 Worker 线程。
#[derive(Default)]
pub struct JsRuntimePool {
    /// Worker 名称 → 句柄的映射表，RwLock 保护并发读写
    workers: RwLock<HashMap<String, Arc<RuntimeWorkerHandle>>>,
}

/// `RwLock` 中毒恢复：读锁。
fn recover_read(
    lock: &RwLock<HashMap<String, Arc<RuntimeWorkerHandle>>>,
) -> std::sync::RwLockReadGuard<'_, HashMap<String, Arc<RuntimeWorkerHandle>>> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!("workers RwLock poisoned during read, recovering");
        e.into_inner()
    })
}

/// `RwLock` 中毒恢复：写锁。
fn recover_write(
    lock: &RwLock<HashMap<String, Arc<RuntimeWorkerHandle>>>,
) -> std::sync::RwLockWriteGuard<'_, HashMap<String, Arc<RuntimeWorkerHandle>>> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!("workers RwLock poisoned during write, recovering");
        e.into_inner()
    })
}

impl JsRuntimePool {
    /// 创建空的运行时池。
    #[must_use]
    pub fn new() -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
        }
    }

    /// 在池中执行脚本，若 Worker 不存在则自动创建。
    ///
    /// - `script_name` —— 脚本名称，同时作为 Worker 的唯一标识
    /// - `bytecode` —— `QuickJS` 字节码
    /// - `run_type` —— 运行模式
    /// - `params` —— 调用参数
    /// - `env` —— 环境变量
    /// - `runtime_clean_time_ms` —— 空闲清理时间阈值（ms），None 表示永不清理
    /// - `limits` —— 运行时资源限制
    ///
    /// `limits` 来自 `js_worker` 表的 `max_run_time` / `max_stack_size` / `max_heap_size`。
    /// heap/stack 在 Worker 首次创建时固定，`update.rs` 调用 `evict_worker` 强制下次
    /// 重建时采用新值；`max_run_time_ms` 每次调用生效。
    ///
    /// # Errors
    /// 若 Worker 通道已关闭或脚本执行失败，返回错误。
    #[allow(clippy::too_many_arguments)]
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

    /// 获取或初始化 Worker，使用 double-check locking 避免重复创建。
    ///
    /// 将 `spawn_worker()` 移入写锁内部，避免并发时创建后被丢弃的 Worker。
    #[allow(clippy::significant_drop_tightening)]
    fn get_or_init_worker(
        &self,
        script_name: &str,
        limits: RuntimeLimits,
    ) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
        debug!(target: "js_runtime", script_name = %script_name, "getting or initializing worker");
        {
            let workers = recover_read(&self.workers);
            if let Some(worker) = workers.get(script_name).cloned() {
                return Ok(worker);
            }
        }

        {
            let mut workers = recover_write(&self.workers);

            // 写锁内最终检查，防止并发创建
            if let Some(existing) = workers.get(script_name).cloned() {
                return Ok(existing);
            }

            // 在写锁内 spawn，避免竞态创建后丢弃
            let worker = spawn_worker(script_name, limits)?;
            workers.insert(script_name.to_owned(), Arc::clone(&worker));
            Ok(worker)
        }
    }

    /// 清理空闲超时的 Worker。
    ///
    /// 条件：`runtime_clean_time > 0`、无活跃请求、无外部 Arc 引用、
    /// 空闲时长超过清理阈值。
    pub fn cleanup_idle_workers(&self) {
        let now = get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp during runtime cleanup");
            0
        });

        let candidates: Vec<String> = {
            let workers = recover_read(&self.workers);
            workers
                .iter()
                .filter_map(|(name, worker)| {
                    let clean_ms = worker.runtime_clean_time()?;

                    if clean_ms <= 0 {
                        return None;
                    }

                    if worker.active_requests.load(Ordering::Acquire) > 0 {
                        return None;
                    }

                    if Arc::strong_count(worker) > 1 {
                        return None;
                    }

                    let last_used = worker.last_used_ms.load(Ordering::Acquire);
                    if now.saturating_sub(last_used) >= clean_ms {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        if candidates.is_empty() {
            return;
        }

        let mut workers = recover_write(&self.workers);

        for name in candidates {
            let should_remove = workers.get(&name).is_some_and(|worker| {
                let Some(clean_ms) = worker.runtime_clean_time() else {
                    return false;
                };

                if clean_ms <= 0 {
                    return false;
                }

                if worker.active_requests.load(Ordering::Acquire) > 0 {
                    return false;
                }

                if Arc::strong_count(worker) > 1 {
                    return false;
                }

                let last_used = worker.last_used_ms.load(Ordering::Acquire);
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

    /// 强制驱逐指定 Worker，发送 Shutdown 命令并从池中移除。
    ///
    /// update 操作后调用此方法，确保下次执行时使用新配置重建 Worker。
    pub fn evict_worker(&self, script_name: &str) -> bool {
        let removed = {
            let mut workers = recover_write(&self.workers);
            workers.remove(script_name)
        };

        removed.is_some_and(|worker| {
            debug!(target: "js_runtime", worker_name = %script_name, "Evicting JS runtime worker");
            let _ = worker.sender.send(WorkerCommand::Shutdown);
            true
        })
    }

    /// 获取运行时池状态快照，用于 `get_rt_pool` RPC。
    #[must_use]
    pub fn snapshot(&self) -> RuntimePoolInfo {
        let now = get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp during runtime snapshot");
            0
        });
        let workers = {
            let guard = recover_read(&self.workers);
            guard
                .values()
                .map(|worker| {
                    let last_used = worker.last_used_ms.load(Ordering::Acquire);
                    RuntimePoolWorkerInfo {
                        script_name: worker.script_name.clone(),
                        active_requests: worker.active_requests.load(Ordering::Acquire),
                        last_used_ms: last_used,
                        idle_ms: now.saturating_sub(last_used),
                        runtime_clean_time_ms: worker.runtime_clean_time(),
                    }
                })
                .collect::<Vec<_>>()
        };

        RuntimePoolInfo {
            total_workers: workers.len(),
            workers,
        }
    }
}

/// 全局运行时池单例。
static GLOBAL_RUNTIME_POOL: OnceLock<Arc<JsRuntimePool>> = OnceLock::new();
/// 标记清理循环是否已启动，防止重复 spawn。
static CLEANUP_LOOP_STARTED: AtomicBool = AtomicBool::new(false);

/// 获取全局运行时池单例，懒初始化。
#[must_use]
pub fn global_pool() -> &'static Arc<JsRuntimePool> {
    GLOBAL_RUNTIME_POOL.get_or_init(|| {
        info!(target: "js_runtime", "initializing global JS runtime pool");
        Arc::new(JsRuntimePool::new())
    })
}

/// 初始化全局运行时池并启动周期性清理循环。
///
/// 必须在服务器启动时调用一次。清理循环每隔 1 秒扫描空闲 Worker。
pub fn init_global_pool() -> &'static Arc<JsRuntimePool> {
    let pool = global_pool();

    if !CLEANUP_LOOP_STARTED.swap(true, Ordering::AcqRel) {
        let pool_for_task = Arc::clone(pool);
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_millis(CLEANUP_INTERVAL_MS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                pool_for_task.cleanup_idle_workers();
            }
        });
    }

    pool
}

/// 创建新的 Worker 线程，返回其句柄。
///
/// Worker 线程命名为 `js-rt-{script_name}`，内部运行 `worker_loop` 接收命令。
fn spawn_worker(
    script_name: &str,
    limits: RuntimeLimits,
) -> anyhow::Result<Arc<RuntimeWorkerHandle>> {
    debug!(target: "js_runtime", script_name = %script_name, max_run_time_ms = limits.max_run_time_ms, max_stack_size_bytes = limits.max_stack_size_bytes, max_heap_size_bytes = limits.max_heap_size_bytes, "spawning new worker thread");
    let script_name = script_name.to_owned();
    let (tx, rx) = std::sync::mpsc::sync_channel::<WorkerCommand>(256);

    let handle = Arc::new(RuntimeWorkerHandle {
        script_name: script_name.clone(),
        sender: tx,
        active_requests: AtomicUsize::new(0),
        last_used_ms: AtomicI64::new(get_local_timestamp_ms_i64().unwrap_or_else(|e| {
            warn!(target: "js_runtime", error = %e, "Failed to read local timestamp when spawning runtime worker");
            0
        })),
        runtime_clean_time_ms: AtomicI64::new(RUNTIME_CLEAN_TIME_NONE),
        last_bytecode_hash: AtomicU64::new(0),
    });

    std::thread::Builder::new()
        .name(format!("js-rt-{script_name}"))
        .spawn(move || worker_loop(&script_name, rx, limits))
        .map_err(|e| anyhow::anyhow!("Failed to spawn JS runtime worker thread: {e}"))?;

    Ok(handle)
}

/// Worker 线程主循环。
///
/// 创建 current-thread Tokio Runtime，循环接收 `WorkerCommand`：
/// - `Execute`：在 Runtime 上执行脚本，通过 oneshot 通道返回结果
/// - `Shutdown`：退出循环，线程结束
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
    // 缓存上次发送的 bytecode Arc，避免相同字节码重复传输
    let mut cached_bytecode: Option<Arc<Vec<u8>>> = None;

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
                // bytecode: Some → 更新缓存；None → 复用上次
                let effective_bytecode = match bytecode {
                    Some(bc) => {
                        cached_bytecode = Some(Arc::clone(&bc));
                        bc
                    }
                    None => cached_bytecode
                        .clone()
                        .unwrap_or_else(|| Arc::new(Vec::new())),
                };
                let exec_result = host_rt.block_on(async {
                    execute_on_worker(
                        &mut runtime_state,
                        script_name,
                        &effective_bytecode,
                        bytecode_hash,
                        run_type,
                        &params,
                        &env,
                        limits,
                        max_run_time_ms,
                    )
                    .await
                    .map_err(|e| format_js_error(&e))
                });
                let _ = response_tx.send(exec_result);
            }
            WorkerCommand::Shutdown => {
                drop(runtime_state.take());
                let () = host_rt.block_on(async {
                    tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;
                });
                break;
            }
        }
    }
}

/// 在 Worker 线程内执行一次脚本。
///
/// 内部步骤：
/// 1. 若 `RuntimeState` 不存在，创建新的（含 `QuickJS` Runtime/Context、资源限制、kill handler）
/// 2. 清除上次执行残留的 `kill_flag`
/// 3. 若字节码哈希变化，重新加载字节码模块并设置 `__nodeget_entry` 全局变量
/// 4. 启动硬超时看门狗（OS 线程 + interrupt handler）
/// 5. 设置调用参数全局变量，执行 `INVOKE_SCRIPT_JS` IIFE
/// 6. 等待结果或超时
/// 7. 清除所有定时器，检查是否被硬超时打断
/// 8. 若被硬超时打断：丢弃整个 RuntimeState（残留未清理的 promise 会影响后续执行）
/// 9. 等待 `rt.idle()` 确保 `QuickJS` GC 完成（100ms 超时兜底）
/// 10. drain I/O 清理窗口（10ms），让 hyper 连接 task 处理关闭信号
#[allow(
    clippy::future_not_send,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
async fn execute_on_worker(
    runtime_state: &mut Option<RuntimeState>,
    script_name: &str,
    bytecode: &[u8],
    bytecode_hash: u64,
    run_type: RunType,
    params: &Value,
    env: &Value,
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
    state.kill_flag.store(false, Ordering::Release);

    // 字节码哈希不同时需要重新加载模块
    if state.loaded_bytecode_hash != Some(bytecode_hash) {
        let load_result: Result<(), Error> = state
            .ctx
            .async_with(async |ctx| {
                let declared_module = enrich_exception(&ctx, "js_load", unsafe {
                    Module::load(ctx.clone(), bytecode)
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
    let cancel_tx = register_watchdog(Arc::clone(&state.kill_flag), effective_timeout);

    let run_future = state.ctx.async_with(async |ctx| {
        prepare_invoke_globals(
            &ctx,
            run_type.handler_name(),
            params,
            env,
            Some(script_name),
            None,
            0,
        )?;

        let invoke_promise: Promise<'_> =
            enrich_exception(&ctx, "js_invoke", ctx.eval(INVOKE_SCRIPT_JS))?;
        let js_value: JsValue<'_> = enrich_exception(
            &ctx,
            "js_invoke",
            invoke_promise.into_future::<JsValue<'_>>().await,
        )?;

        let result = resolve_invoke_result(&ctx, js_value);

        // 执行完成后清理全局变量，释放 JS 堆内存。
        // Pool 路径复用 AsyncContext，不清理会导致 params/env 留在 globalThis 上无法 GC。
        // 注意：不能清理 __nodeget_entry，因为 pool 路径只在 bytecode 变化时重新加载模块，
        // __nodeget_entry 需要保留供后续执行使用（one-shot 路径可以清理，因为 Runtime 用完即弃）。
        ctx.eval::<(), _>(
            "globalThis.__nodeget_run_params = null;\
             globalThis.__nodeget_env = null;\
             globalThis.inlineCall = null;\
             globalThis.__nodeget_inline_caller = null;",
        )
        .ok();

        result
    });
    let run_outcome: Result<Value, Error> = tokio::time::timeout(effective_timeout, run_future)
        .await
        .unwrap_or_else(|_| Err(js_error("js_runner", "JavaScript execution timed out")));
    let _ = cancel_tx.send(());

    // 判定是否是因为硬超时被 interrupt 打断。interrupt 会让 QuickJS 抛不可
    // 捕获异常，但 runtime 内部可能仍残留 pending jobs / 待清理的 promise
    // reactions。pool 场景下这个 AsyncRuntime 之后会继续服务新请求，残留
    // 状态可能影响下一次执行——最稳的做法是丢弃当前 state，下次调用走
    // `create_runtime_state` 重建一个干净的 runtime。
    let killed_by_timeout = state.kill_flag.load(Ordering::Acquire) && run_outcome.is_err();

    // 重置 kill_flag，避免 interrupt handler 打断后续的定时器清理操作。
    // killed 状态已保存在 `killed_by_timeout` 变量中。
    if killed_by_timeout {
        state.kill_flag.store(false, Ordering::Release);
    }

    // 清除所有定时器，防止 idle() 因未清理的 setInterval 挂起，
    // 同时清理 RT_TIMER_STATE 条目以避免潜在的 Runtime 销毁问题。
    // 此时 kill_flag 已为 false，interrupt handler 不会干扰。
    // 同时检查 fetch 使用标志，用于条件性 I/O drain。
    let fetch_used: bool = state.ctx.async_with(async |ctx| {
        let _ = ctx.eval::<(), _>(
            "if(typeof globalThis.__nodeget_clear_all_timers==='function')globalThis.__nodeget_clear_all_timers()"
        );
        let used: bool = ctx
            .eval::<bool, _>("globalThis.__nodeget_fetch_used === true")
            .unwrap_or(false);
        let _ = ctx.eval::<(), _>("globalThis.__nodeget_fetch_used = false");
        used
    }).await;

    if killed_by_timeout {
        // Drop 整个 runtime state：释放 AsyncContext + AsyncRuntime，
        // QuickJS 释放所有 JS 对象（含未消费 Incoming body 的 Response），
        // 向 hyper 连接 task 发出异步关闭信号。
        *runtime_state = None;

        // drop 后关闭信号已发出，需要 runtime 继续轮询才能处理。
        // 否则被中断的 fetch 连接将永远停留在 CLOSE_WAIT。
        tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;

        return Err(js_error(
            "js_runner",
            format!("JavaScript execution exceeded max_run_time_ms={max_run_time_ms}"),
        ));
    }

    // 有界 idle —— 安全兜底：若清理遗漏了持久异步工作，
    // 100ms 超时触发并丢弃运行时状态。
    let idle_ok = tokio::time::timeout(std::time::Duration::from_millis(100), state.rt.idle())
        .await
        .is_ok();

    if !idle_ok {
        // idle() 超时 —— 运行时存在无法终止的持久工作，丢弃状态
        *runtime_state = None;
        tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;
        return Err(js_error(
            "js_runner",
            "Runtime cleanup timed out — state discarded",
        ));
    }

    // 条件性 I/O drain：仅在本次执行使用了 fetch() 时等待。
    //
    // JS 执行期间 fetch() 产生的 Response 对象可能未被完全消费（未调
    // .text()/.json() 等）。rt.idle() 期间 QuickJS GC 可能回收了部分
    // Response，其持有的 hyper Incoming body 被 drop，向连接 task 发出
    // 异步关闭信号。但 pool worker 的 current_thread runtime 在
    // block_on 返回后不再被轮询，关闭信号将无法处理——TCP 连接停留在
    // CLOSE_WAIT（远端已 FIN，本地未 FIN，Recv-Q 有残留字节）。
    //
    // 无 fetch 时跳过 drain，快速脚本（1-10ms）吞吐量提升 10-100 倍。
    // 有 fetch 时 100ms (DRAIN_IO_MS) 足以覆盖绝大多数 HTTP 连接关闭握手。
    if fetch_used {
        tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;
    }

    run_outcome
}

/// 创建新的 `RuntimeState`，包含 `QuickJS` Runtime/Context 及全局初始化。
///
/// 内部步骤：
/// 1. 创建 `AsyncRuntime` 并应用 heap/stack 限制
/// 2. 安装 interrupt handler（共享 `kill_flag`）
/// 3. 创建 `AsyncContext` 并初始化全局 API（nodeget、fetch、execSql 等）
/// 4. 等待 `rt.idle()` 确保初始化完成
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

/// 计算字节切片的哈希值，用于判断字节码是否变化。
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}
