//! 服务器端 `QuickJS` 运行时核心 —— JS 执行引擎、资源限制、字节码编译。
//!
//! 提供：
//! - `RuntimeLimits` —— 运行时资源限制（执行时长、栈大小、堆大小）
//! - `js_runner` / `js_runner_source_mode` —— 一次性 JS 执行（用完即弃 Runtime）
//! - `compile_js_module_to_bytecode` —— JS 模块编译为 `QuickJS` 字节码
//! - `init_js_runtime_globals` —— 注入 `nodeget()`、`fetch`、`execSql` 等全局 API
//! - `register_watchdog` —— 常驻看门狗线程管理器，打断 CPU 密集的 JS 无限循环
//!
//! 与 `runtime_pool` 模块的区别：此模块的执行器创建临时 Runtime，执行完毕后销毁；
//! `runtime_pool` 维护持久化的 Worker 池，字节码缓存避免重复加载。

use crate::{JsCodeInput, RunType};
use rquickjs::prelude::{Async, Func};
use rquickjs::{
    AsyncContext, AsyncRuntime, Ctx, Error, Module, Promise, Value as JsValue, WriteOptions,
};
use serde_json::Value;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, mpsc};
use tracing::{debug, error, trace};
use uuid::Uuid;

/// `QuickJS` 运行时默认内存上限（8 MiB）。
pub(crate) const JS_RT_MEMORY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// `max_run_time` 的应用层默认值（ms）。见 `js_worker.max_run_time`，NULL 时兜底。
pub const DEFAULT_MAX_RUN_TIME_MS: u64 = 30_000;
/// `max_stack_size` 的应用层默认值（bytes）。QuickJS 本身默认 256 KiB，我们提到 1 MiB。
pub const DEFAULT_MAX_STACK_SIZE_BYTES: usize = 1024 * 1024;
/// `max_heap_size` 的应用层默认值（bytes）。与历史常量 `JS_RT_MEMORY_LIMIT_BYTES` 一致。
pub const DEFAULT_MAX_HEAP_SIZE_BYTES: usize = JS_RT_MEMORY_LIMIT_BYTES;

/// I/O drain 窗口（ms）。一次性 Runtime 的 current-thread 在 `block_on` 返回后
/// 不再被轮询，此窗口让 hyper 连接 task 处理关闭信号，防止 TCP 停留在 `CLOSE_WAIT`。
const DRAIN_IO_MS: u64 = 100;

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
/// 抓不住，脚本会被真正终止。我们把 flag 拿在外面：看门狗常驻线程超时后
/// `store(true)`，`QuickJS` 下一个检查点就被打断。
///
/// 这样即便脚本里写的是 `while(true){}` 这种无 await 纯 CPU 循环，也能被杀。
pub(crate) async fn install_kill_handler(rt: &AsyncRuntime, kill_flag: Arc<AtomicBool>) {
    rt.set_interrupt_handler(Some(Box::new(move || kill_flag.load(Ordering::Relaxed))))
        .await;
}

// ── 全局看门狗管理器 ────────────────────────────────────────────────
// 每次 JS 执行不再 spawn/join 一个独立 OS 线程，而是向常驻看门狗线程注册
// 监控请求。避免高频执行场景下反复创建销毁 OS 线程的开销。
//
// 看门狗线程循环：
// 1. 非阻塞 try_recv 收集所有 Register 请求
// 2. 找到最近的 deadline，sleep 到该 deadline（或被新 Register 唤醒）
// 3. 检查所有已到期请求，设置 kill_flag
// 4. 清理已取消的请求（cancel_tx drop 后 cancel_rx 返回 Disconnected）

/// 看门狗线程中一条活跃监控记录。
struct ActiveWatch {
    deadline_ms: u64,
    kill_flag: Arc<AtomicBool>,
    /// 当 `cancel_tx` drop 时，`try_recv` 返回 `Disconnected`，表示执行完成、取消监控。
    cancel_rx: mpsc::Receiver<()>,
}

/// 看门狗请求：向常驻看门狗线程注册一条监控。
struct WatchdogRegister {
    deadline_ms: u64,
    kill_flag: Arc<AtomicBool>,
    cancel_rx: mpsc::Receiver<()>,
}

/// 全局看门狗管理器，持有向常驻看门狗线程发送注册请求的通道。
struct WatchdogManager {
    sender: mpsc::Sender<WatchdogRegister>,
}

impl WatchdogManager {
    /// 注册一个看门狗监控请求，返回 `cancel_tx`。
    /// 执行完成后 `drop` `cancel_tx` 或 `send(())` 即可取消监控。
    fn register(
        &self,
        kill_flag: Arc<AtomicBool>,
        duration: std::time::Duration,
    ) -> mpsc::Sender<()> {
        let deadline_ms = now_ms() + duration.as_millis() as u64;
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>();
        // 注册失败（看门狗线程已退出）不影响正确性——看门狗是尽力辅助
        let _ = self.sender.send(WatchdogRegister {
            deadline_ms,
            kill_flag,
            cancel_rx,
        });
        cancel_tx
    }
}

/// 当前时间戳（ms since epoch），用于 deadline 计算。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 启动常驻看门狗线程，返回管理器。
fn init_watchdog_manager() -> WatchdogManager {
    let (req_tx, req_rx) = mpsc::channel::<WatchdogRegister>();

    std::thread::Builder::new()
        .name("js-watchdog-manager".to_owned())
        .spawn(move || {
            let mut active: Vec<ActiveWatch> = Vec::new();

            loop {
                // 1. 非阻塞收集所有新注册请求
                loop {
                    match req_rx.try_recv() {
                        Ok(reg) => active.push(ActiveWatch {
                            deadline_ms: reg.deadline_ms,
                            kill_flag: reg.kill_flag,
                            cancel_rx: reg.cancel_rx,
                        }),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }

                // 2. 清理已取消的请求（cancel_tx 被 drop -> Disconnected）
                active.retain(|w| w.cancel_rx.try_recv() != Err(mpsc::TryRecvError::Disconnected));

                // 3. 找到最近的 deadline
                let nearest = active.iter().map(|w| w.deadline_ms).min();

                // 4. Sleep 到最近的 deadline（或短间隔以检查新请求）
                let sleep_ms = match nearest {
                    Some(d) => (d.saturating_sub(now_ms())).min(50),
                    None => 50, // 无活跃请求，短轮询间隔
                };
                if sleep_ms > 0 {
                    let _ = req_rx.recv_timeout(std::time::Duration::from_millis(sleep_ms));
                }

                // 5. 检查已到期的请求，设置 kill_flag 并移除
                let now = now_ms();
                active.retain(|w| {
                    if w.deadline_ms <= now {
                        w.kill_flag.store(true, Ordering::Relaxed);
                        false
                    } else {
                        true
                    }
                });
            }
        })
        .expect("failed to spawn js-watchdog-manager OS thread");

    WatchdogManager { sender: req_tx }
}

/// 全局看门狗管理器单例。
static WATCHDOG_MANAGER: OnceLock<WatchdogManager> = OnceLock::new();

/// 注册看门狗监控请求。返回 `cancel_tx`——执行完成后 `drop` 或 `send(())` 取消监控。
///
/// 语义与原 `spawn_kill_watchdog` 一致：到时间仍未被 cancel 就 `store(true)`。
/// 区别是使用常驻看门狗线程，不再每次 spawn/join。
pub(crate) fn register_watchdog(
    kill_flag: Arc<AtomicBool>,
    duration: std::time::Duration,
) -> mpsc::Sender<()> {
    let manager = WATCHDOG_MANAGER.get_or_init(init_watchdog_manager);
    manager.register(kill_flag, duration)
}

/// 构造一个带有阶段标识的 `rquickjs::Error`，用于统一错误来源标记。
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

/// `init_js_runtime_globals` 中注入的 JS 包装代码。
///
/// 约 100 行 JS，每次创建 `QuickJS` 上下文时都要 `eval`。`rquickjs` 不直接支持
/// `Ctx::eval_bytecode()`，因此暂时保留 `ctx.eval()`。未来可改用 Module
/// 字节码预编译路径来消除重复解析开销。
static GLOBALS_JS: &str = r#"
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
// Timer tracking: wrap setTimeout/setInterval/setImmediate to track IDs
// for cleanup after handler execution (prevents idle() hang from uncleared timers).
const __nodeget_timer_ids = [];
const __origST = globalThis.setTimeout;
const __origSI = globalThis.setInterval;
const __origSIM = globalThis.setImmediate;
globalThis.setTimeout = (cb, delay, ...args) => {
    const id = __origST(cb, delay, ...args);
    __nodeget_timer_ids.push(id);
    return id;
};
globalThis.setInterval = (cb, delay, ...args) => {
    // Enforce minimum 4ms interval (browser spec), prevents 250Hz CPU burn
    const id = __origSI(cb, Math.max(delay || 0, 4), ...args);
    __nodeget_timer_ids.push(id);
    return id;
};
globalThis.setImmediate = (cb, ...args) => {
    const id = __origSIM(cb, ...args);
    __nodeget_timer_ids.push(id);
    return id;
};
globalThis.__nodeget_clear_all_timers = () => {
    let limit = 100;
    while (__nodeget_timer_ids.length > 0 && limit-- > 0) {
        const ids = __nodeget_timer_ids.splice(0);
        for (const id of ids) { clearTimeout(id); clearInterval(id); }
    }
};
globalThis.db = {
    async create(token, name) {
        const resp = await nodeget("db_create", { token, name });
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
};
// fetch 使用追踪：包装原生 fetch，记录是否被调用，用于条件性 I/O drain
const __nodeget_orig_fetch = globalThis.fetch;
if (__nodeget_orig_fetch) {
    globalThis.fetch = function(...args) {
        globalThis.__nodeget_fetch_used = true;
        return __nodeget_orig_fetch.apply(this, args);
    };
}
"#;

/// 初始化 JS 运行时全局 API。
///
/// 注入的 API 包括：
/// 1. `llrt_fetch` / `llrt_buffer` / `llrt_stream_web` / `llrt_url` / `llrt_util` / `llrt_timers`
/// 2. `__nodeget_rpc_raw` —— 原始 JSON-RPC 调用（返回 JSON 字符串）
/// 3. `__nodeget_inline_call_raw` —— 原始内联调用（返回 JSON 字符串）
/// 4. `randomUUID` —— UUID 生成
/// 5. `nodeget()` —— 封装后的 JSON-RPC 调用（返回解析后的 JS 对象）
/// 6. `__nodeget_inline_call()` —— 封装后的内联调用
/// 7. `execSql()` —— 数据库 SQL 执行
/// 8. `getDatabaseType()` —— 获取数据库类型
/// 9. 定时器追踪 —— 包装 setTimeout/setInterval/setImmediate，支持 `__nodeget_clear_all_timers`
/// 10. `db.*` —— 数据库 CRUD 操作快捷方式
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
    // JS 代码已提取到 GLOBALS_JS 静态常量，避免每次内联大段字符串
    ctx.eval::<(), _>(GLOBALS_JS)?;
    Ok(())
}

/// 从 JS 上下文提取异常信息，组装包含名称、消息和堆栈的可读字符串。
///
/// `QuickJS` 的 `.stack` 只包含调用帧不含错误消息，需要手动拼接。
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

/// 将 JS 异常类型的 `rquickjs::Error` 替换为包含详细堆栈信息的错误。
///
/// 若 `result` 是 `Error::Exception`，从上下文提取异常信息；
/// 否则原样返回。
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

/// 将 JS 源码编译为 `QuickJS` 模块字节码，不执行模块。
///
/// 内部步骤：
/// 1. 检查脚本和文件名不含 NUL 字节
/// 2. 声明 ES 模块
/// 3. 序列化为字节码（`Module::write`）
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

/// 将 JS 模块编译为 `QuickJS` 字节码。
///
/// 创建临时 current-thread Tokio Runtime 和 `QuickJS` Runtime/Context，
/// 在上下文中初始化全局 API（与运行时保持一致），然后编译模块。
///
/// # Errors
/// 若 JS 模块编译失败，返回错误。
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
                // 编译上下文与运行时上下文保持一致，确保全局 API 可用
                init_js_runtime_globals(&ctx)?;

                compile_module_bytecode_no_eval(&ctx, &js_code)
            })
            .await;

        rt.idle().await;
        compile_result
    })
}

/// 执行脚本的 IIFE 模板：读取 `__nodeget_*` 全局变量，调用 handler，返回结果。
///
/// 此为共享的 JS 代码片段，在通过 [`prepare_invoke_globals`] 设置全局变量、
/// `__nodeget_entry` 从模块命名空间设置后运行。
pub const INVOKE_SCRIPT_JS: &str = r#"
(async () => {
    // Reset timer tracking for this execution
    if (globalThis.__nodeget_timer_ids) globalThis.__nodeget_timer_ids.length = 0;
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

/// 在 JS 上下文中设置五个 `__nodeget_*` 全局变量，供脚本执行模板使用。
///
/// - `__nodeget_run_handler` —— handler 函数名字符串（如 `"onCall"`）
/// - `__nodeget_run_params` —— 调用参数（通过 `json_parse` 转为 JS 对象）
/// - `__nodeget_env` —— 环境变量（通过 `json_parse` 转为 JS 对象）
/// - `__nodeget_current_script_name` —— 脚本名称字符串或 `null`
/// - `__nodeget_inline_caller` —— 内联调用方名称字符串或 `null`
///
/// # Errors
/// 若任何全局变量设置失败（序列化或 JS 引擎错误），返回错误。
pub fn prepare_invoke_globals(
    ctx: &Ctx<'_>,
    run_type: &str,
    params: &Value,
    env: &Value,
    script_name: Option<&str>,
    inline_caller: Option<&str>,
) -> Result<(), Error> {
    let global = ctx.globals();

    global.set("__nodeget_run_handler", run_type.to_owned())?;

    let params_json = serde_json::to_string(params).map_err(|e| {
        js_error(
            "js_runner",
            format!("Failed to serialize input params: {e}"),
        )
    })?;
    let params_js = ctx.json_parse(params_json).map_err(|e| {
        js_error(
            "js_runner",
            format!("Failed to build input params in JS: {e}"),
        )
    })?;
    global.set("__nodeget_run_params", params_js)?;

    let env_json = serde_json::to_string(env)
        .map_err(|e| js_error("js_runner", format!("Failed to serialize env: {e}")))?;
    let env_js = ctx
        .json_parse(env_json)
        .map_err(|e| js_error("js_runner", format!("Failed to build env in JS: {e}")))?;
    global.set("__nodeget_env", env_js)?;

    match script_name {
        Some(name) => global.set("__nodeget_current_script_name", name.to_owned())?,
        None => {
            global.set(
                "__nodeget_current_script_name",
                JsValue::new_null(ctx.clone()),
            )?;
        }
    }

    match inline_caller {
        Some(caller) => global.set("__nodeget_inline_caller", caller.to_owned())?,
        None => {
            global.set("__nodeget_inline_caller", JsValue::new_null(ctx.clone()))?;
        }
    }

    Ok(())
}

/// 执行完成后清理 `prepare_invoke_globals` 设置的全局变量，释放 JS 堆内存。
///
/// 将 `__nodeget_run_params`、`__nodeget_env` 等大对象设为 `null`，
/// 让 `QuickJS` GC 在 `idle` 阶段回收这些引用的数据。
fn cleanup_invoke_globals(ctx: &Ctx<'_>) {
    // 清理大对象全局变量，释放 JS 堆内存；失败时静默忽略（Runtime 即将销毁）
    ctx.eval::<(), _>(
        r"globalThis.__nodeget_run_params = null;
        globalThis.__nodeget_env = null;
        globalThis.__nodeget_entry = null;
        globalThis.inlineCall = null;
        globalThis.__nodeget_inline_caller = null;",
    )
    .ok();
}

/// 将 JS 返回值转换为 `serde_json::Value`。
///
/// 处理三种情况：
/// - `undefined` —— 视为错误（脚本必须返回 JSON 可序列化值）
/// - JS 字符串 —— 直接解析为 JSON
/// - 其他类型 —— 通过 `json_stringify` 序列化后再解析
///
/// # Errors
/// 若值为 `undefined`、不可 JSON 序列化或序列化结果非合法 JSON，返回错误。
pub fn resolve_invoke_result<'js>(ctx: &Ctx<'js>, js_value: JsValue<'js>) -> Result<Value, Error> {
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
}

/// 一次性 JS 执行器（字节码或源码模式），执行完毕后销毁 Runtime。
///
/// - `js_code` —— JS 代码输入（源码或字节码）
/// - `run_type` —— 运行模式
/// - `input_params` —— 调用参数
/// - `env_value` —— 环境变量
/// - `current_script_name` —— 当前脚本名称（用于日志和错误追踪）
/// - `inline_caller` —— 内联调用方名称
/// - `caller_soft_timeout` —— 调用方软超时（`inline_call` 的 timeoutSec）
/// - `limits` —— 运行时资源限制
///
/// `limits` 来自 `js_worker` 表，控制 heap / stack / 执行时长硬上限。
/// `caller_soft_timeout` 是 `inline_call` / 调用方传入的软超时；与 `limits.max_run_time_ms`
/// 取较严的一个作为最终 `tokio::time::timeout` 时长。
///
/// # Errors
/// 若构建宿主 Runtime 或 JS 执行失败，返回错误。
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
            let js_result: Result<Value, Error> = ctx
                .async_with(async |ctx| {
                    init_js_runtime_globals(&ctx)?;
                    prepare_invoke_globals(
                        &ctx,
                        run_type.handler_name(),
                        &input_params,
                        &env_value,
                        current_script_name.as_deref(),
                        inline_caller.as_deref(),
                    )?;

                    let declared_module = match &js_code {
                        JsCodeInput::Source(source) => enrich_exception(
                            &ctx,
                            "js_load",
                            Module::declare(
                                ctx.clone(),
                                "js_worker.js",
                                source.as_bytes().to_vec(),
                            ),
                        )?,
                        JsCodeInput::Bytecode(bytecode) => {
                            enrich_exception(&ctx, "js_load", unsafe {
                                Module::load(ctx.clone(), bytecode)
                            })?
                        }
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
                    ctx.globals().set("__nodeget_entry", entry_value)?;

                    let invoke_promise: Promise<'_> =
                        enrich_exception(&ctx, "js_invoke", ctx.eval(INVOKE_SCRIPT_JS))?;
                    let js_value: JsValue<'_> = enrich_exception(
                        &ctx,
                        "js_invoke",
                        invoke_promise.into_future::<JsValue<'_>>().await,
                    )?;

                    let result = resolve_invoke_result(&ctx, js_value);

                    // 执行完成后清理全局变量，释放 JS 堆内存
                    cleanup_invoke_globals(&ctx);

                    result
                })
                .await;

            // 有界 idle：一次性 Runtime 执行后即销毁，但未清理的 setInterval
            // 仍可能让 idle() 永远挂起。50ms 足够 GC 完成。
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rt.idle()).await;
            js_result
        };

        // 硬超时路径：常驻看门狗线程 + interrupt handler 打断 CPU 循环，
        // 外层 tokio::time::timeout 捕捉 async 路径上的挂起（await 点停在
        // 远端 I/O 等）。两层共同保障 max_run_time_ms 兜得住。
        let cancel_tx = register_watchdog(Arc::clone(&kill_flag), effective_timeout);
        let outcome = match tokio::time::timeout(effective_timeout, execute).await {
            Ok(result) => result,
            Err(_) => Err(js_error("js_runner", "JavaScript execution timed out")),
        };

        // 执行完成或超时后，取消看门狗监控（常驻线程自动清理，无需 join）
        let _ = cancel_tx.send(());

        // 释放 QuickJS 上下文——触发 GC 释放所有 JS 对象，包括 fetch()
        // 产生的 Response（其 Incoming body 可能未被 JS 代码消费）。Drop
        // Incoming 向 hyper 连接 task 发出异步关闭信号。
        drop(ctx);

        // drain I/O 清理窗口：current_thread runtime 在 block_on 返回后不再被轮询，
        // fetch() 产生的 Response Incoming body drop 向 hyper 连接 task 发出异步关闭信号，
        // 需要 runtime 继续运转才能处理，否则 TCP 连接停留在 CLOSE_WAIT。
        tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;

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

/// 源码模式一次性 JS 执行器，语义与 [`js_runner`] 一致。
///
/// 与 `js_runner` 的区别：
/// - 始终使用源码模式声明模块（`Module::declare`），模块名使用 `{script_name}.js` 以改善堆栈追踪
/// - 无字节码缓存路径
///
/// # Errors
/// 若构建宿主 Runtime 或 JS 执行失败，返回错误。
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
            let js_result: Result<Value, Error> = ctx
                .async_with(async |ctx| {
                    init_js_runtime_globals(&ctx)?;
                    prepare_invoke_globals(
                        &ctx,
                        run_type.handler_name(),
                        &input_params,
                        &env_value,
                        Some(script_name),
                        None,
                    )?;

                    // 使用实际脚本名作为模块名，改善错误堆栈追踪
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
                    ctx.globals().set("__nodeget_entry", entry_value)?;

                    let invoke_promise: Promise<'_> =
                        enrich_exception(&ctx, "js_invoke", ctx.eval(INVOKE_SCRIPT_JS))?;
                    let js_value: JsValue<'_> = enrich_exception(
                        &ctx,
                        "js_invoke",
                        invoke_promise.into_future::<JsValue<'_>>().await,
                    )?;

                    let result = resolve_invoke_result(&ctx, js_value);

                    // 执行完成后清理全局变量，释放 JS 堆内存
                    cleanup_invoke_globals(&ctx);

                    result
                })
                .await;

            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), rt.idle()).await;
            js_result
        };

        let cancel_tx = register_watchdog(Arc::clone(&kill_flag), effective_timeout);
        let outcome = match tokio::time::timeout(effective_timeout, execute).await {
            Ok(result) => result,
            Err(_) => Err(js_error("js_runner", "JavaScript execution timed out")),
        };
        let _ = cancel_tx.send(());

        // 同 js_runner()：释放 QuickJS 上下文以 drop 未消费的 fetch Response
        // Incoming body，drain I/O 清理窗口防止 CLOSE_WAIT。
        drop(ctx);
        tokio::time::sleep(std::time::Duration::from_millis(DRAIN_IO_MS)).await;

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
