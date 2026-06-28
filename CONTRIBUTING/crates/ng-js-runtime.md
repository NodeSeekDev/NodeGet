# ng-js-runtime

> 概览：ng-js-runtime 提供 NodeGet JS Worker 的 QuickJS JavaScript 执行引擎。它维护一个按脚本名索引的专用 OS 线程池，每个线程承载一个常驻的 QuickJS `AsyncRuntime` / `AsyncContext`，并额外提供用于 bytecode / source 的一次式执行器。该 crate 向每个 JS 上下文注入 `nodeget()`（JSON-RPC 桥）、`inlineCall()`（递归上限 10）、`fetch`、`execSql`、`getDatabaseType`、`db.*`、定时器以及基于 `tracing` 的 `nodegetLog`。常驻 watchdog 线程配合 QuickJS interrupt handler 强杀 CPU 死循环；bytecode 按哈希缓存以避免重复编译。通过 `default = []`（仅类型）与 `server` feature 的标准门控，并以 OnceLock 注入 `JsWorkerService` trait 和 `spawn_on_server_runtime` 作为接缝，使 JS 回调能在不与 ng-js-worker 形成循环依赖的前提下访问 server 持有的资源（DB pool、RpcModule）。

## 模块结构

| 文件 | 角色 |
|------|------|
| `crates/ng-js-runtime/src/lib.rs` | Crate 根。配置 clippy lint；声明常驻 `types` 模块（glob 再导出），并通过 `#[cfg(feature = "server")]` 条件编译其余执行模块；再导出公共 server runtime API。 |
| `crates/ng-js-runtime/src/types.rs` | 默认 feature 类型定义：`RunType`、`CompileMode`、`JsCodeInput`、`RuntimePoolWorkerInfo`、`RuntimePoolInfo`，含单元测试。 |
| `crates/ng-js-runtime/src/runtime_pool.rs` | 常驻 QuickJS runtime 池：每个注册脚本名一个专用 OS 线程 + `AsyncRuntime`/`AsyncContext`。worker 通过有界 `std::sync::mpsc` 通道接收 `Execute`/`Shutdown` 命令；bytecode 按哈希缓存以跳过重复编译。提供空闲清理、强制驱逐（用于 worker 配置更新）以及 `get_rt_pool` RPC 使用的快照。 |
| `crates/ng-js-runtime/src/server_runtime.rs` | 核心 QuickJS 执行引擎。定义 `RuntimeLimits`（运行时 / 栈 / 堆上限，DB 驱动的 `from_model` 与 `effective_timeout`）、常驻 watchdog 线程（通过 interrupt handler 强杀 CPU 死循环）、bytecode 编译、JS globals 引导（`nodeget`/`fetch`/`execSql`/`db.*`/`nodegetLog`/定时器跟踪）以及两个一次式执行器。 |
| `crates/ng-js-runtime/src/inline_call.rs` | 实现 `__nodeget_inline_call_raw` JS 全局：将 inline worker 调用转发给注入的 `JsWorkerService`。params 与结果以原始 JSON 字符串传递（不重新解析）。 |
| `crates/ng-js-runtime/src/nodeget.rs` | 实现 `__nodeget_rpc_raw` JS 全局：将 JS worker 内部的 JSON-RPC 请求派发到 server 的 `RpcModule`。检测批量请求（起始 `[`）并在单次 server-runtime 跳转内派发所有子请求。 |
| `crates/ng-js-runtime/src/js_worker_service.rs` | 依赖注入接缝。定义 `JsWorkerService` trait 与 `RawJsonDispatcher` trait（不依赖 jsonrpsee 的抽象），并提供 OnceLock 全局的 set/get，切断与 ng-js-worker 的循环依赖。 |
| `crates/ng-js-runtime/src/spawn_on_server_runtime.rs` | 将 worker current-thread runtime 上的 future 桥接到常驻 server tokio Runtime。DB/RPC 资源绑定在 server executor 上，故必须如此。提供 `init`（Handle 注入）、`spawn_on_server_runtime` 与 `AbortOnDrop`。 |

## 公共 API

### 类型（默认 feature，始终可用）

| 名称 | 签名 | 行为 |
|------|------|------|
| `RunType` | `pub enum RunType { Call, Cron, Route, InlineCall }`（`#[serde(rename_all = "snake_case")]`） | 决定 `INVOKE_SCRIPT_JS` 调用哪个 handler；序列化为 `call`/`cron`/`route`/`inline_call`。 |
| `RunType::as_str` | `pub const fn as_str(&self) -> &'static str` | 返回 `call`/`cron`/`route`/`inline_call`。 |
| `RunType::handler_name` | `pub const fn handler_name(&self) -> &'static str` | 返回 `onCall`/`onCron`/`onRoute`/`onInlineCall`。 |
| `CompileMode` | `pub enum CompileMode { Bytecode (default), Source }`（serde snake_case） | 默认 Bytecode；在 js-worker 中选择执行路径。 |
| `JsCodeInput` | `pub enum JsCodeInput { Source(String), Bytecode(Vec<u8>) }`（Debug + Clone，非 serde） | 在 `js_runner` 中选择 source-vs-bytecode 执行。 |
| `RuntimePoolWorkerInfo` | `pub struct { script_name: String, active_requests: usize, last_used_ms: i64, idle_ms: i64, runtime_clean_time_ms: Option<i64> }`（Serialize + Deserialize） | 每个 worker 的快照字段。 |
| `RuntimePoolInfo` | `pub struct { total_workers: usize, workers: Vec<RuntimePoolWorkerInfo> }`（Serialize + Deserialize） | 整池快照，供 `get_rt_pool` RPC 使用。 |

### Server feature 下的执行与编译

| 名称 | 签名 | 行为 |
|------|------|------|
| `RuntimeLimits::from_model` | `pub fn from_model(max_run_time_ms: Option<i64>, max_stack_size_bytes: Option<i64>, max_heap_size_bytes: Option<i64>) -> Self` | 映射 DB 字段：`Some(n>0)` -> `n`（`try_from`，溢出回退默认），其余（NULL / 非正 / 越界）一律回退 `DEFAULT_*`。 |
| `RuntimeLimits::effective_timeout` | `pub fn effective_timeout(self, caller_soft_timeout: Option<Duration>) -> Duration` | hard = `max_run_time_ms`；给出 soft 时返回 `hard.min(soft)`，否则 hard。 |
| `compile_js_module_to_bytecode` | `pub fn compile_js_module_to_bytecode(js_code: impl AsRef<str>) -> Result<Vec<u8>, Error>` | 启 current_thread runtime + QuickJS，初始化 globals（与执行路径一致），`Module::declare` 后 `module.write(WriteOptions::default())` 序列化 bytecode；拒绝 NUL 字节。 |
| `js_runner` | `pub fn js_runner(js_code: JsCodeInput, run_type, input_params, env_value, current_script_name: Option<String>, inline_caller: Option<String>, inline_depth: u32, caller_soft_timeout: Option<Duration>, limits: RuntimeLimits) -> Result<Value, Error>` | 一次式执行器：加载 source 或 bytecode、eval、设置 `__nodeget_entry`、运行 `INVOKE_SCRIPT_JS`、返回序列化 `Value`。watchdog + interrupt handler 强超时；成功路径按 `__nodeget_fetch_used` 条件触发 `DRAIN_IO_MS` 排空，JS 执行错误或外层 timeout 时会保守地同样执行一次 drain。 |
| `js_runner_source_mode` | `pub fn js_runner_source_mode(source_code: &str, script_name: &str, run_type, input_params, env_value, caller_soft_timeout, limits) -> Result<Value, Error>` | 形态同 `js_runner`，但始终 source 模式、模块名 `{script_name}.js`（更好的栈追踪）、`inline_caller=None`、`inline_depth=0`。 |
| `js_error` | `pub fn js_error(stage: &'static str, message: impl Into<String>) -> Error` | 构造带 stage 标签的 `Error::new_from_js_message`（from = stage，type tag = `String`）。 |
| `format_js_error` | `pub fn format_js_error(&Error) -> String` | 对带非空 message 的 `Error::FromJs` 返回 `[from] msg`，否则 Display。 |
| `prepare_invoke_globals` | `pub fn prepare_invoke_globals(ctx: &Ctx<'_>, run_type: &str, params: &Value, env: &Value, script_name: Option<&str>, inline_caller: Option<&str>, inline_depth: u32) -> Result<(), Error>` | 设置 5 个 `__nodeget_*` 调用全局 + inline depth。 |
| `resolve_invoke_result<'js>` | `pub fn resolve_invoke_result(ctx: &Ctx<'js>, js_value: JsValue<'js>) -> Result<Value, Error>` | undefined -> error；JS string -> JSON parse；否则 json_stringify + serde parse；function/symbol -> error。 |

### 池与注入 API（server feature）

| 名称 | 签名 | 行为 |
|------|------|------|
| `JsRuntimePool::execute_script` | `pub async fn execute_script(&self, script_name: &str, bytecode: Vec<u8>, run_type, params, env, runtime_clean_time_ms: Option<i64>, limits: RuntimeLimits) -> anyhow::Result<Value>` | 池入口：`get_or_init_worker`（heap/stack 在创建时固定），逐调用设置 clean time 与 `max_run_time_ms`；bytecode 按哈希缓存。 |
| `JsRuntimePool::evict_worker` | `pub fn evict_worker(&self, script_name: &str) -> bool` | 移除命名 worker 并发送 `Shutdown`；存在则返回 true。配置更新后用于强制重建。 |
| `JsRuntimePool::snapshot` | `pub fn snapshot(&self) -> RuntimePoolInfo` | 读锁下读取全部 worker 生成快照。 |
| `global_pool` | `pub fn global_pool() -> &'static Arc<JsRuntimePool>` | 惰性初始化全局池（不启动清理任务）。 |
| `init_global_pool` | `pub fn init_global_pool() -> &'static Arc<JsRuntimePool>` | 返回全局池，并恰好一次地启动清理 ticker（5s 间隔）。 |
| `set_js_worker_service` / `get_js_worker_service` | `pub fn set_js_worker_service(Box<dyn JsWorkerService>); pub fn get_js_worker_service() -> Option<&'static dyn JsWorkerService>` | 幂等注入；first-write-wins。必须在 server 启动时调用。 |
| `init_server_runtime`（即 `spawn_on_server_runtime::init`） | `pub fn init(handle: tokio::runtime::Handle)` | 幂等注入 server tokio Handle。first-write-wins。 |
| `spawn_on_server_runtime` | `pub async fn spawn_on_server_runtime<F, T>(future: F) -> Result<T, String> where F: Future<Output=T>+Send+'static, T: Send+'static` | 在 server runtime 上 spawn 并 await；通过 `AbortOnDrop` 传播调用方取消；Handle 未注入时报错。 |
| `JsWorkerService`（trait） | `pub trait JsWorkerService: Send+Sync+'static { fn run_inline_call_and_record_result(..) -> Pin<Box<dyn Future<Output=anyhow::Result<String>>+Send>>; fn get_rpc_module() -> Pin<Box<dyn Future<Output=Box<dyn RawJsonDispatcher+Send>>+Send>> }` | 由 ng-js-worker 实现，启动时注入。 |
| `RawJsonDispatcher`（trait） | `pub trait RawJsonDispatcher: Send+Sync { fn raw_json_request(&self, json: &str, buf_size: usize) -> Pin<Box<dyn Future<Output=anyhow::Result<(String,())>>+Send+'_>> }` | 对 jsonrpsee `RpcModule` 的抽象，使本 crate 不直接依赖 jsonrpsee。返回元组中的 `()` 是被丢弃的通知流。 |

## 关键类型与常量

### 常量（`server_runtime.rs`、`nodeget.rs`）

| 常量 | 行 | 值 / 说明 |
|------|----|-----------|
| `JS_RT_MEMORY_LIMIT_BYTES` | `crates/ng-js-runtime/src/server_runtime.rs:27` | `pub(crate) const ... : usize = 8 MiB`，用于编译路径，等于 `DEFAULT_MAX_HEAP_SIZE_BYTES`。 |
| `DEFAULT_MAX_RUN_TIME_MS` | `crates/ng-js-runtime/src/server_runtime.rs:30` | `pub const ... : u64 = 30_000`。 |
| `DEFAULT_MAX_STACK_SIZE_BYTES` | 同上 | `1 MiB`。 |
| `DEFAULT_MAX_HEAP_SIZE_BYTES` | 同上 | `8 MiB`。 |
| `MAX_INLINE_DEPTH` | （`GLOBALS_JS` 内） | `10`，JS 侧 `__nodeget_inline_call` 强制，Rust 侧 `inline_depth` 透传，二者必须同步。 |
| `RPC_BUF_SIZE` | `crates/ng-js-runtime/src/nodeget.rs:16` | `const ... : usize = 4096`，传给 `raw_json_request` 作为 jsonrpsee `raw_json_request` 内部通知/订阅流 channel 容量；不是 RPC 响应体大小上限。 |
| `DRAIN_IO_MS` | `crates/ng-js-runtime/src/runtime_pool.rs:34` 等处引用 | `100ms`；pool 正常路径仅在 `__nodeget_fetch_used` 为真时触发，硬超时 / idle-cleanup timeout 会额外 sleep 一次；一次式执行器成功时按 fetch 标志决定，JS 执行错误或外层 timeout 时则保守地同样执行一次 drain。 |
| Cleanup ticker 间隔 | `crates/ng-js-runtime/src/runtime_pool.rs:442` | `5_000ms`，`MissedTickBehavior::Delay`。 |
| `SyncSender` 容量 | `crates/ng-js-runtime/src/runtime_pool.rs`（`RuntimeWorkerHandle`） | `std::sync::mpsc::sync_channel(256)`。 |
| `RUNTIME_CLEAN_TIME_NONE` | `crates/ng-js-runtime/src/runtime_pool.rs:95` | `-1`，负值 load 视为 `None`。 |
| Watchdog 死线扫描上限 | `crates/ng-js-runtime/src/server_runtime.rs:201` | 每 `<=50ms` 一轮扫描。 |
| `__nodeget_clear_all_timers` 上限 | `GLOBALS_JS` | 最多 100 次迭代。 |
| `setInterval` 最小间隔钳制 | `GLOBALS_JS` | 钳到 `>=4ms`。 |

### 关键类型

| 类型 | 行 | 说明 |
|------|----|------|
| `RuntimeLimits` | `crates/ng-js-runtime/src/server_runtime.rs:46` | `pub` 字段 `max_run_time_ms: u64`、`max_stack_size_bytes: usize`、`max_heap_size_bytes: usize`；`Clone + Copy + Debug`；`defaults()` / `Default` 委托到三个 `DEFAULT_*`。 |
| `RuntimeLimits::effective_timeout` | `crates/ng-js-runtime/src/server_runtime.rs:97` | `hard = from max_run_time_ms`；给出 soft 时 `hard.min(soft)`。 |
| `apply_runtime_limits` | `crates/ng-js-runtime/src/server_runtime.rs:117` | `pub(crate) async fn`：`set_memory_limit(heap)` 与 `set_max_stack_size(stack)`；必须在首次脚本执行前调用。 |
| `install_kill_handler` | `crates/ng-js-runtime/src/server_runtime.rs:131` | `set_interrupt_handler` 返回 `kill_flag.load(Relaxed)`，true 时 QuickJS 抛出不可捕获异常，连 `while(true){}` 也能杀掉。 |
| `WorkerCommand` | `crates/ng-js-runtime/src/runtime_pool.rs:55` | `enum { Execute{ bytecode: Option<Arc<Vec<u8>>>, bytecode_hash: u64, run_type, params: Arc<Value>, env: Arc<Value>, max_run_time_ms: u64, response_tx: oneshot::Sender<Result<Value,String>> }, Shutdown }`。`bytecode=None` 表示复用缓存。 |
| `RuntimeWorkerHandle` | `crates/ng-js-runtime/src/runtime_pool.rs:79` | 字段：`script_name:String`、`sender: SyncSender<WorkerCommand>`（256）、`active_requests:AtomicUsize`、`last_used_ms:AtomicI64`、`runtime_clean_time_ms:AtomicI64`（负哨兵 = 从未清理）、`last_bytecode_hash:AtomicU64`（0 = 从未发送）。 |
| `ActiveRequestGuard<'a>` | `crates/ng-js-runtime/src/runtime_pool.rs:188` | `struct(&'a AtomicUsize)`；`Drop` 以 `fetch_sub AcqRel` 递减，即便提前返回或 panic 也保证 `active_requests` 递减。 |
| `JsRuntimePool` | `crates/ng-js-runtime/src/runtime_pool.rs:197` | `struct { workers: RwLock<HashMap<String, Arc<RuntimeWorkerHandle>>> }`；`#[derive(Default)]`。 |
| `recover_read` / `recover_write` | `crates/ng-js-runtime/src/runtime_pool.rs:204` | `unwrap_or_else` 将 poison 映射为 `into_inner` 并 warn，使池在锁中毒时仍存活而不 panic。 |
| `GLOBALS_JS` | `crates/ng-js-runtime/src/server_runtime.rs:334` | 约 100 行 JS：将 `__nodeget_rpc_raw` 包装为 `nodeget()`；定义 `__nodeget_inline_call`（强制 `MAX_INLINE_DEPTH=10`）；`execSql`/`getDatabaseType`（调用 nodeget RPC）；包装 `setTimeout`/`setInterval`（钳到 `>=4ms`）/`setImmediate` 跟踪 timer ID；`__nodeget_clear_all_timers`（上限 100）；`db.{create,read,update,remove,list,execSql}`；包装 `fetch` 设置 `__nodeget_fetch_used`；`nodegetLog`/`__nodeget_log` 桥接。 |
| `INVOKE_SCRIPT_JS` | `crates/ng-js-runtime/src/server_runtime.rs:640` | IIFE 模板：每次执行开始时仅重置 timer ID 数组；构造 `inlineCall`（校验 worker 名非空、timeout 正有限、`JSON.stringify` 参数、调用 `__nodeget_inline_call`）；设置 `globalThis.inlineCall`；校验入口是 object 且 handler 存在；`onRoute`：从输入构造 `Request`（url、method、headers、`body_base64` 经 `atob`），要求返回 `Response` 并序列化 status/headers/body_base64；其余 handler：`await handler(input, env, runtimeCtx)`，返回 undefined 报错。 |
| `ActiveWatch` / `WatchdogRegister` / `WatchdogManager` | `crates/ng-js-runtime/src/server_runtime.rs:147` | `ActiveWatch { deadline_ms: u64, kill_flag: Arc<AtomicBool>, cancel_rx: mpsc::Receiver<()> }`；`WatchdogRegister` 同字段并被线程消费；`WatchdogManager { sender }`，`register` 返回 `cancel_tx`，drop 或 `send(())` 取消。 |

## 内部机制

### 常驻 watchdog 线程 + interrupt 硬杀

单个常驻 OS 线程 `js-watchdog-manager` 持有 `Vec<ActiveWatch>`。每次 JS 执行调用 `register_watchdog(kill_flag, duration)`，通过 mpsc 发送一个 `WatchdogRegister` 并返回 `cancel_tx`。线程每 `<=50ms` 一轮：排空新注册、保留未断连的 watch、找到最近死线、最多睡眠 50ms、对过期 watch 设置 `kill_flag=true` 并丢弃。每个 `AsyncRuntime` 上安装的 interrupt handler 在 QuickJS 检查点轮询 `kill_flag`，true 即抛出不可捕获异常。drop 或 `send(())` 即移除 watch。pool 与一次式路径共享此机制（`crates/ng-js-runtime/src/server_runtime.rs:201-265`、`crates/ng-js-runtime/src/runtime_pool.rs:648`）。

### Bytecode 哈希缓存

每个 worker 用 `DefaultHasher`（非密码学，仅用于检测变更）对 bytecode 求哈希。若哈希匹配 handle 上的 `last_bytecode_hash`，`execute` 发送 `bytecode:None`，worker 复用已缓存 module 跳过 `Module::load`；不匹配则发送新的 `Arc<Vec<u8>>`，worker 重新加载并设置 `__nodeget_entry`。`last_bytecode_hash` 在 Acquire/Release 序下更新。pool 路径有意不在执行之间清理 `__nodeget_entry` 以支持复用（`crates/ng-js-runtime/src/runtime_pool.rs:122-185`、`536-643`）。

### 并发：双重检查锁 + 两遍空闲清理

`get_or_init_worker` 使用 read-then-write 双重检查，并在 write 锁内 spawn worker，避免竞争后丢弃 worker。`cleanup_idle_workers` 两遍扫描：读锁下收集候选（`clean_ms>0`、`active_requests==0`、`Arc::strong_count==1`、`idle>=threshold`），再写锁下逐个重新校验后才 remove + 发 `Shutdown`。RwLock poison 被恢复（`into_inner`）而非传播（`crates/ng-js-runtime/src/runtime_pool.rs:204-222`、`272-298`、`304-376`）。

### CLOSE_WAIT 缓解：条件式 I/O 排空

worker 线程运行 current_thread Tokio runtime；`block_on` 返回后不再被 poll。`fetch()` 可能留下未消费的 hyper `Incoming` body；`rt.idle()` 的 GC 可能丢弃 `Response`，其异步 close 信号需要被 poll。为避免 TCP `CLOSE_WAIT`，pool 路径在正常完成后仅在 `__nodeget_fetch_used` 被设置时才条件式睡眠 `DRAIN_IO_MS`（100ms）；若硬超时或 idle-cleanup timeout 导致丢弃 `RuntimeState`，也会在返回前额外 sleep 一次。一次式执行器成功时根据 `__nodeget_fetch_used` 决定是否 drain，但在 JS 执行报错或外层 timeout 时会因无法可靠确认 fetch 使用情况而保守地同样 sleep 一次。

### JS 回调的 server-runtime 桥接

JS 全局 `__nodeget_rpc_raw`（Async `js_nodeget`）与 `__nodeget_inline_call_raw`（Async `js_inline_call`）都把函数体包进 `spawn_on_server_runtime`，使 DB pool / RpcModule 访问运行在常驻 server Runtime 而非 worker 短命的 current_thread runtime 上。server-runtime Handle 必须在启动时经 `init()` 注入，否则这些调用报错 `server runtime handle is not initialized`。取消经 `AbortOnDrop` 传播——drop 时若 JoinHandle 未完成则 abort（`crates/ng-js-runtime/src/spawn_on_server_runtime.rs:14-58`、`crates/ng-js-runtime/src/nodeget.rs:40-90`、`crates/ng-js-runtime/src/inline_call.rs:38-53`）。

### 批量 RPC 派发（RawValue）

批量请求以起始 `[` 检测，解析为 `Vec<Box<RawValue>>` 以避免逐项 parse/serialize；一次 `get_rpc_module()` 调用对所有项复用。单请求在 trim 未缩减时复用原 `String`，否则分配裁剪后的切片。inline call 的 params / 结果同样以原始 JSON 字符串透传（`crates/ng-js-runtime/src/nodeget.rs:34-90`、`crates/ng-js-runtime/src/inline_call.rs`）。

### 全局单例与清理任务生命周期

清理 ticker（`5_000ms`，`MissedTickBehavior::Delay`）通过 `CLEANUP_LOOP_STARTED` 的 `AtomicBool::swap(AcqRel)` 最多启动一次。`GLOBAL_RUNTIME_POOL` 是 `OnceLock<Arc<JsRuntimePool>>`。`WATCHDOG_MANAGER`、`SERVER_RUNTIME_HANDLE`、`JS_WORKER_SERVICE` 同为 OnceLock 单例，启动时注入（`crates/ng-js-runtime/src/runtime_pool.rs:426-459`）。

### 一次式 vs 常驻 runtime 分流

`js_runner` 与 `js_runner_source_mode` 每次调用都构建全新 current_thread Tokio Runtime + `AsyncRuntime` + `AsyncContext` 并在之后丢弃。pool 路径（`execute_on_worker`）将 `RuntimeState` 跨调用按脚本名保活，在更新、空闲超时或硬杀后驱逐。pool 跨执行复用 `__nodeget_entry`；一次式路径经 `cleanup_invoke_globals` 清理它（`crates/ng-js-runtime/src/server_runtime.rs:817-827`、`884-1030`；`crates/ng-js-runtime/src/runtime_pool.rs:591-766`）。

### 定时器跟踪与 setInterval 钳制

`GLOBALS_JS` 包装 `setTimeout`/`setInterval`/`setImmediate` 把 ID 记入 `__nodeget_timer_ids`；pool 路径在执行后会显式调用 `__nodeget_clear_all_timers`（上限 100 次迭代），而 `INVOKE_SCRIPT_JS` 仅在每次执行开始时把 timer ID 数组长度重置为 0。一次式执行器依赖 `cleanup_invoke_globals`、有界 `rt.idle()` 与 runtime drop 做收尾，而不是显式调用 `__nodeget_clear_all_timers`。`setInterval` 钳到 `>=4ms` 以防 CPU 烧毁。

## Crate 内部约定

- **Lint 门控**：crate 根 `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`，全局允许 `cast_sign_loss`、`cast_precision_loss`、`cast_possible_truncation`、`similar_names`（`crates/ng-js-runtime/src/lib.rs:1-7`）。
- **Feature 门控**：`default = []` 仅暴露类型（`types.rs` 经 `pub use types::*`）；`server` feature 门控所有执行模块、runtime 池、注入 trait 与公共执行器（`crates/ng-js-runtime/src/lib.rs:31-54`）。默认 feature 因而可供非 server 侧 crate 仅复用类型，不要求像 agent 这样具体消费者一定直接依赖本 crate。
- **日志 target**：`js_runtime` 用于内部 Rust/QuickJS 生命周期；`js_worker` 用于经 `nodegetLog`/`__nodeget_log` 桥接的 JS 日志输出（`crates/ng-js-runtime/src/server_runtime.rs:302-327`）。
- **Serde**：enum 使用 `#[serde(rename_all = "snake_case")]`；inline-call 面向 worker 的参数以原始 JSON 字符串（不解析）传递，避免冗余 parse/serialize 往返（`inline_call.rs`、`nodeget.rs`）。
- **QuickJS 全局命名空间**：私有前缀 `__nodeget_*`；用户可见别名 `nodeget`、`inlineCall`、`execSql`、`getDatabaseType`、`db.*`、`fetch`、`randomUUID`、`nodegetLog`、`setTimeout`/`setInterval`/`setImmediate`。
- **中文注释**：内联注释与模块 docstring 以中文描述架构。
- **自定义 jsonrpsee fork**：命名空间使用 `_` 分隔符（注入 JS 中可见 `nodeget-server_exec_sql`、`db_create`、`db_exec_sql`）。
- **OnceLock 注入模式（四处）**：全局 runtime 池、全局 watchdog manager、全局 server-runtime Handle、`JsWorkerService` 实现。
- **RwLock poison 恢复**：经 `recover_read`/`recover_write` 恢复而非 panic（`runtime_pool.rs`）。
- **时间跟踪**：worker 空闲记账使用 `ng_core::utils::get_local_timestamp_ms_i64`（返回 `i64`）；watchdog 死线使用 `now_ms()`（`SystemTime`，`u64`）。

## 注意事项与陷阱

- **维护者必须在 worker 配置变更后调用 `evict_worker`**：`crates/ng-js-runtime/src/runtime_pool.rs:381`。Heap 与 stack 上限在 worker 创建时**固定**，只有 `max_run_time_ms` 逐调用生效。当 js-worker 的 `max_stack_size`/`max_heap_size` 变更时，更新路径必须调用 `evict_worker(name)`，否则旧上限静默保留。
- **切勿依赖 `set_js_worker_service` / `init(handle)` 报错**：`crates/ng-js-runtime/src/js_worker_service.rs:59`。`set_js_worker_service` 静默忽略第二次调用（`let _ = .set`），first-write-wins；若启动注入错误实现或注入两次，后者被无声丢弃。`init(handle)` 同模式。
- **`RuntimeLimits::from_model` 静默回退默认**：`crates/ng-js-runtime/src/server_runtime.rs:58`。任何非正或越界（`usize`/`u64` 溢出）的 DB 值都被视为「用默认」而非报错。误配的 `max_run_time=0` 或负值会静默变成 `30000ms`，运维方无法察觉其 limit 被拒绝。
- **worker 队列有界 `sync_channel(256)`，过载即拒**：`crates/ng-js-runtime/src/runtime_pool.rs:156`。`try_send` 失败向调用方（`execute_script`）返回 anyhow 错误 `Worker queue full, request rejected`。在单个 worker 名下重负载时，第 257 个并发请求被直接拒绝——背压以请求失败而非排队的形式出现。
- **watchdog 线程 spawn 是 fatal panic**：`crates/ng-js-runtime/src/server_runtime.rs:204`。线程创建失败时 `.expect('failed to spawn js-watchdog-manager OS thread')` 触发进程级 panic；其余所有线程 spawn 都映射到 `Result`，唯独此处是致命的。
- **`Module::load(bytecode)` 是 `unsafe` 且 bytecode 受信任**：`crates/ng-js-runtime/src/runtime_pool.rs:619`。`compile_js_module_to_bytecode` 总是在编译前初始化 globals，使 bytecode 引用同一套 globals；但加载任意 / 不可信 bytecode 会导致内存不安全。缓存哈希是非密码学 `DefaultHasher`，不防御对抗性碰撞。
- **inlineCall 递归上限 JS 与 Rust 必须同步**：`crates/ng-js-runtime/src/server_runtime.rs:351`。`MAX_INLINE_DEPTH=10` 在 JS（`GLOBALS_JS` 的 `__nodeget_inline_call`）强制，`inline_depth` 又经 `prepare_invoke_globals`/`run_inline_call_and_record_result`（Rust 侧）透传。两侧必须保持同步——只放宽 JS 常量而不动 Rust 管线（或反之）会形成绕过。每层子调用 `+1`。
- **`onRoute` 必须返回 `Response`，其余 handler 必须返回可 JSON 序列化值**：`crates/ng-js-runtime/src/server_runtime.rs:725`。`onRoute` handler 必须返回 `Response`（`instanceof Response`），否则 IIFE 抛错；`onCall`/`onCron`/`onInlineCall` 必须返回可 JSON 序列化值（undefined 抛 `JS handler must return a JSON-serializable value`）；function/symbol 作为返回值经 `resolve_invoke_result` 报错。
- **硬超时杀掉会丢弃 worker 全局状态**：`crates/ng-js-runtime/src/runtime_pool.rs:722`。pool 路径硬超时后整个 `RuntimeState` 被丢弃（drop），下次调用重建全新 QuickJS runtime——脚本设置的任何全局状态都会丢失。一次式执行器本就销毁 runtime。硬超时返回前会 sleep 一次 `DRAIN_IO_MS`，但下次创建新 runtime 时不会再补一次同类 drain。
- **`RPC_BUF_SIZE=4096` 是传给 jsonrpsee `raw_json_request` 的 channel 容量，不是响应体上限**：`crates/ng-js-runtime/src/nodeget.rs:16`。当前 server 适配层把它原样传给 `RpcModule::raw_json_request(..., buf_size)`；在所用 jsonrpsee 实现中，这个值用于内部 `mpsc::channel(buf_size)`，而 `max_response_size` 仍是 `usize::MAX`。因此不要把它理解成 `nodeget()` / `execSql` RPC 响应固定 4096 字节上限。
- **两条路径的 `__nodeget_*` 全局清理范围不同**：`crates/ng-js-runtime/src/runtime_pool.rs:671`。pool 路径有意**不**清理 `__nodeget_entry`（以便缓存 module 复用），但**会**清理 `__nodeget_run_params`/`env`/`inlineCall`/`inline_caller`；一次式路径（`cleanup_invoke_globals`）额外清理 `__nodeget_entry`。在两条路径间混用这些全局，或在一次式模式下依赖 `__nodeget_entry` 持久化，都是 bug。
- **空闲驱逐 `Arc::strong_count` 是竞态启发式**：`crates/ng-js-runtime/src/runtime_pool.rs:325`。检查 `strong_count > 1` 以跳过有在途调用方的 worker，但 `strong_count` 有竞态、仅为启发式——一个正要抓取 Arc 的调用方可能被漏掉，但随后的 `execute` 会发现 worker 已被移除并重建它（正确性保留，只是浪费一次驱逐）。

## 依赖关系

本 crate 是 JS 执行的底层引擎，被上层 `ng-js-worker`（CRUD、执行服务、RPC）依赖；server 二进制在启动时向本 crate 注入 `JsWorkerService`（实现在 ng-js-worker）、server tokio `Handle` 与全局池。本 crate 反向依赖 `ng-core`（错误类型、`utils::get_local_timestamp_ms_i64`）以及外部 `rquickjs`/QuickJS 绑定与 `llrt_*` 系列 Web 原语 crate。通过 `RawJsonDispatcher` trait 抽象掉对 jsonrpsee 的直接依赖，并由 `spawn_on_server_runtime` 将 worker 短命 runtime 上的回调桥接到 server Runtime。
