# ng-js-worker —— JS Worker 记录管理与 RPC 命名空间

> 概览：`ng-js-worker` 在 `ng-js-runtime`（QuickJS 线程池）之上提供 JS Worker 脚本的 CRUD、执行调度与结果记录，并暴露 `js-worker` / `js-result` 两个 RPC 命名空间。它本身不持有 QuickJS 实例，而是通过 `service.rs` 的三个入口把执行委托给运行时池或一次性 Runtime，并把每次执行的开始/结束状态写入 `js_result` 表。所有权限校验委托给 `ng_core::permission::permission_checker`。

## 模块结构

```
crates/ng-js-worker/src/
├── lib.rs                # Crate 根：feature 门控、合并 js-worker + js-result 两个 module
├── service.rs            # 执行服务：字节码/源码/内联调用三个入队入口 + 字节码版本校验
├── auth.rs               # RBAC 权限校验辅助（Worker 级、Result 级、列表过滤）
├── js_worker/
│   ├── mod.rs            # #[rpc(server, namespace="js-worker")] trait + impl + rpc_module()
│   ├── auth.rs           # 仅从 crate::auth 重导出（保持 handler include 路径一致）
│   ├── create.rs         # js-worker_create：编译字节码 + 入库
│   ├── update.rs         # js-worker_update：重编译 + 驱逐旧 Runtime
│   ├── delete.rs         # js-worker_delete：删库 + 驱逐 Runtime
│   ├── read.rs           # js-worker_read：读详情（含 js_script_base64，不含字节码）
│   ├── run.rs            # js-worker_run：按 CompileMode 分派到字节码/源码入队
│   ├── get_rt_pool.rs    # js-worker_get_rt_pool：运行时池快照
│   ├── list_all_js_worker.rs  # js-worker_list_all_js_worker：列出可见 Worker 名
│   └── route_name.rs     # route_name 归一化（防路径遍历）
└── js_result/
    ├── mod.rs            # #[rpc(server, namespace="js-result")] trait + impl + rpc_module()
    ├── query.rs          # js-result_query：按条件查询（默认 LIMIT 1000，上限 10000）
    └── delete.rs         # js-result_delete：按条件删除（Limit/Last 走 select-id-then-delete）
```

## Crate 根约定

- `#![cfg_attr(feature = "server", allow(clippy::too_many_arguments))]`（`lib.rs:13`）—— create/update 的参数多达 10 个，crate 级豁免该 lint。
- 默认 feature 无任何类型导出（Worker 类型即 `ng-db` 中的数据库实体）。`server` feature 下才编译 `auth` / `service` / `js_worker` / `js_result` 模块。
- `rpc_module()`（`lib.rs:34`）合并 `js-worker` 与 `js-result` 两个命名空间到一个 `RpcModule<JsWorkerRpcImpl>`，供 server binary 一次性注册。`merge` 失败会 panic（`.expect("failed to merge js_result RPC module")`）。

## 公共 API（`server` feature）

| 函数 | 签名 | 行为 |
|------|------|------|
| `enqueue_defined_js_worker_run` | `async (String, RunType, Value, Option<Value>) -> Result<i64>` | 字节码模式入队：查 worker → `ensure_bytecode_version` → 插 `js_result` 行 → `tokio::spawn` 异步执行 → 完成后 update 行。返回 `js_result.id`（`service.rs:118`） |
| `enqueue_source_js_worker_run` | `async (String, RunType, Value, Option<Value>) -> Result<i64>` | 源码模式入队：用一次性 Runtime `js_runner_source_mode` 每次重新编译执行。返回 `js_result.id`（`service.rs:414`） |
| `run_inline_call_and_record_result` | `async (String, String, Option<f64>, Option<String>, u32) -> Result<String>` | 内联调用：`spawn_blocking` + `js_runner` 同步等待结果，返回结果 JSON 字符串。递归深度上限 10（`service.rs:241`） |
| `ensure_bytecode_version` | `async (&js_worker::Model, &DatabaseConnection) -> Result<Vec<u8>>` | 校验存储字节码的首字节版本号与当前 QuickJS 是否匹配；不匹配则用 `js_script` 重编译、写回 DB、驱逐池中旧 worker（`service.rs:50`） |
| `rpc_module` | `() -> RpcModule<JsWorkerRpcImpl>` | 合并 js-worker + js-result 命名空间（`lib.rs:34`） |

## 关键类型与常量

| 项 | 位置 | 说明 |
|----|------|------|
| `JsWorkerRpcImpl` / `JsResultRpcImpl` | `js_worker/mod.rs:100` / `js_result/mod.rs:32` | 空 struct，`impl RpcHelper for _ {}` 提供 blanket 方法；`into_rpc()` 由 `#[rpc]` 宏生成 |
| `MAX_INLINE_DEPTH = 10` | `service.rs:251` | inlineCall 递归深度上限，与 `server_runtime.rs` 的 JS 侧 `MAX_INLINE_DEPTH` 保持一致（Rust 侧硬上限） |
| `JsResultAction { Read, Delete }` | `auth.rs:170` | js_result 操作类型，决定构造 `Permission::JsResult(Read(_))` 还是 `Delete(_)` |
| `current_bc_version` | `service.rs:29` | 取 QuickJS 字节码版本号（字节码首字节）。在 `spawn_blocking` 中编译最小脚本提取，**禁止在 tokio runtime 内直接调用**（`compile_js_module_to_bytecode` 内部 `block_on` 会创建嵌套 runtime 导致 panic）。结果缓存在 `OnceLock<u8>` |
| `token_time_valid` | `auth.rs:31` | 列表过滤路径的补齐校验：读取 token 的 `timestamp_from`/`timestamp_to`，过期或未生效返回 `false`。时间读取失败也返回 `false`（fail-closed） |
| `normalize_route_name` | `route_name.rs:19` | route_name 归一化：`None→None`；trim 后非空；长度 ≤128；仅允许 `[a-zA-Z0-9._-]`；拒绝纯点组合（`.` / `..`） |

## 内部机制

### 三条执行路径的差异

- **字节码模式**（`enqueue_defined_js_worker_run`）：通过 `runtime_pool::global_pool().execute_script(...)` 复用持久化 worker（线程 + QuickJS 实例常驻），字节码缓存避免重复编译。执行是 fire-and-forget：先返回 `js_result.id`，结果在 `tokio::spawn` 内异步回填。
- **源码模式**（`enqueue_source_js_worker_run`）：每次用 `js_runner_source_mode` 创建一次性 Runtime，重新解析编译。无调用方软超时（`None`），效果完全由 `RuntimeLimits.max_run_time_ms` 决定。同样是 fire-and-forget。
- **内联调用**（`run_inline_call_and_record_result`）：**同步等待**结果。`spawn_blocking` 执行 `js_runner`，返回结果 JSON 字符串，**直接透传避免 parse→serialize 往返**（`service.rs:365-368`）。供 `inlineCall()` API 跨 worker 调用。

### js_result 行的生命周期

三条入口都遵循相同流程：插入 `js_result` 行（`start_time` 已填，`finish_time`/`result`/`error_message` 为 `None`）→ 执行 → 用 `update_many().filter(Id.eq(js_result_id))` 回填 `finish_time` + `result` 或 `error_message`。若执行既无结果也无错误，强制写入 `"JavaScript run finished without result or error"`（`service.rs:202`、`service.rs:376`、`service.rs:509`）——保证行不会永远处于 running 状态。

### 字节码版本兼容

QuickJS 的字节码格式首字节是版本号。升级 QuickJS 后，旧 worker 存储的字节码会失效。`ensure_bytecode_version` 在每次执行前比对版本，不匹配时从 `js_script` 重编译、写回 DB、调用 `runtime_pool::global_pool().evict_worker(name)` 驱逐池中已编译的旧实例。快速路径仅在版本匹配时才 clone 字节码 Vec（可能数十 KiB），避免无条件 clone（`service.rs:54-70`）。

### 权限校验的两条路径

`auth.rs` 对单次操作和批量列表采用不同策略：

- **单次操作**（`check_js_worker_permission` / `ensure_js_result_permission`）：走完整 `check_token_limit`（含 `get_token` 的 ct_eq 验证）。
- **批量列表**（`filter_workers_by_list_permission` / `resolve_accessible_js_result_workers`）：先短路超级令牌，否则仅 `get_token` 一次拿到 `Arc<Vec<Limit>>`，对每个 name 用 `ng_token::get::check_limits_cover` 做纯内存匹配，**避免 N 次串行 `check_token_limit`**（每次内部重复 `get_token` + clone limits）。补齐 `token_time_valid` 时间校验，过期/未生效视为无权限（返回空集）。

`build_required_permission`（`auth.rs:178`）按 action 构造 `Permission::JsResult(Read/Delete(worker_name))`。

## RPC 方法

### `js-worker` 命名空间（`js_worker/mod.rs:33`）

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `create` | token, name, description?, js_script_base64, route_name?, runtime_clean_time?, env?, max_run_time?, max_stack_size?, max_heap_size? | `JsWorker::Create` on `Scope::JsWorker(name)` | 校验 name/脚本非空 → base64 解码 + UTF-8 校验 → 检查 name 与 route_name 唯一性 → `spawn_blocking` 编译字节码 → 插入 `js_worker` 表。返回新记录（`create.rs:41`） |
| `update` | 同 create | `JsWorker::Write` | 查现有记录 → route_name 唯一性（排除自身）→ 重编译字节码 → update（含 `update_at`）→ `evict_worker` 驱逐池中旧实例（`update.rs:44`） |
| `delete` | token, name | `JsWorker::Delete` | `delete_many` by name；`rows_affected == 0` 返回 NotFound；成功后 `evict_worker`（`delete.rs:26`） |
| `read` | token, name | `JsWorker::Read` | 返回详情，含 `js_script_base64`（源码 base64），**不含** `js_byte_code`（内部字节码不暴露）（`read.rs:22`） |
| `run` | token, js_script_name, run_type?(默认 Call), params, env?, compile_mode?(默认 Bytecode) | `JsWorker::RunDefinedJsWorker` | 按 `CompileMode` 分派到 `enqueue_defined_js_worker_run` 或 `enqueue_source_js_worker_run`。返回 `{"id": js_result_id}`，结果异步写入（`run.rs:25`） |
| `get_rt_pool` | token | `Scope::Global` + `Permission::NodeGet(GetRtPool)` | 返回 `runtime_pool::global_pool().snapshot()`（每个 worker 的脚本名、活跃请求数、空闲时长等）（`get_rt_pool.rs:15`） |
| `list_all_js_worker` | token | `JsWorker::ListAllJsWorker`（按 name 逐个过滤） | 查所有 worker name → `filter_workers_by_list_permission` 过滤 → 返回可见子集（升序）（`list_all_js_worker.rs:19`） |

### `js-result` 命名空间（`js_result/mod.rs:20`）

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `query` | token, query: `JsResultDataQuery` | `JsResult::Read(worker_name)` 或自动解析可访问 worker | 见下方查询语义（`query.rs:98`） |
| `delete` | token, query: `JsResultDataQuery` | `JsResult::Delete(worker_name)` 或自动解析 | 见下方删除语义（`delete.rs:30`） |

**查询/删除条件语义**（`JsResultQueryCondition`，定义于 `ng-core`）：支持 `Id`、`JsWorkerId`、`JsWorkerName`、`RunType`、`StartTime/FinishTime` 的 From/To/FromTo、`IsSuccess`（result 非空且 error_message 为空）、`IsFailure`（error_message 非空）、`IsRunning`（两者皆空）、`Limit`、`Last`。

- **未指定 worker_name**：调用 `resolve_accessible_js_result_workers` 自动解析 token 可访问的 worker 集合，用 `is_in` 约束。集合为空时 query 返回 `[]`、delete 返回 `deleted: 0`。
- **指定了 worker_name**：对每个 name 串行 `ensure_js_result_permission`。
- **query**：默认 `LIMIT 1000`，`Limit` 上限 `10000`（`query.rs:142,155`）。`Last` 取最近一条（按 start_time desc、id desc）。排序恒为 start_time desc、id desc。
- **delete**：无 Limit/Last 时直接 `delete_many` 按条件删；有 Limit/Last 时**先 select id 再按 id 删除**（`delete.rs:192`），避免条件不精确误删。`Limit` 上限 `10000`，`Last` 删最近一条。

两个命名空间的 handler 都遵循标准错误桥接：`process_logic` 异步块返回 `anyhow::Result`，外层 `match` 用 `anyhow_to_nodeget_error` 转 `ErrorObject::owned(code, msg, None::<()>)`。

## Crate 内部约定

- **Feature 门控**：`default = []`（无类型导出）vs `server`（RPC + service + auth）。`lib.rs` 所有模块均 `#[cfg(feature = "server")]`。
- **Serde**：`RunType`、`CompileMode` 来自 `ng-js-runtime`（`#[serde(rename_all = "snake_case")]`）。handler 参数直接用 `serde_json::Value`（params/env）和 `Option<i64>`（无 serde default，应用代码用 `unwrap_or`）。
- **Tracing target**：worker 操作用 `"js_worker"`，result 操作用 `"js_result"`（`auth.rs` 的 `token_time_valid` 内部用 `"auth"` target）。
- **Base64**：脚本传输统一用 `base64::engine::general_purpose::STANDARD`（`create`/`update` 解码、`read` 编码）。
- **字节码编译必须 `spawn_blocking`**：`compile_js_module_to_bytecode` 绝不能在 tokio runtime 内同步调用（嵌套 `block_on` panic）。
- **handler 文件结构**：每个 RPC 方法一个文件，`mod.rs` 放 `#[rpc]` trait + impl（`token_identity` + `info_span!` + `rpc_exec!`）+ `rpc_module()`，与全项目一致。

## 注意事项与陷阱

- **字节码编译禁止在 async 上下文同步调用**（`service.rs:29`、`create.rs:119`、`update.rs:118`）：必须 `spawn_blocking`，否则 `block_on` 嵌套 runtime 会 panic。新增任何编译路径都要遵守。
- **inlineCall 深度上限是 Rust 侧硬约束**（`service.rs:251`，`MAX_INLINE_DEPTH=10`）：与 `server_runtime.rs` JS 侧检查互为双保险。任一侧改动都要同步另一侧，否则递归可压垮阻塞线程池。
- **`run_inline_call_and_record_result` 是同步等待**：会占用 `spawn_blocking` 线程直到 worker 执行完成。调用方（`inlineCall()`）已对此有预期，切勿在持有锁的状态下调用。
- **update/delete 后必须 `evict_worker`**（`update.rs:143`、`delete.rs:51`、`service.rs:96`）：否则运行时池会继续使用旧的字节码/配置。新增任何修改 worker 的路径都要驱逐。
- **`js_result` 行必须终止**：三条入口都强制写入 result 或 error_message（`service.rs:202/376/509`）。若新增执行路径，务必保证回填逻辑，否则行永远停在 running。
- **源码模式无调用方软超时**（`service.rs:479` 传 `None`）：执行时长完全由 `RuntimeLimits.max_run_time_ms` 决定。不要误以为 `run_type` 的超时对源码模式生效。
- **`run` 默认值**：`run_type` 默认 `Call`，`compile_mode` 默认 `Bytecode`（`run.rs:34-35`）。客户端省略字段时按此分派。
- **route_name 路径安全**（`route_name.rs`）：字符集白名单 `[a-zA-Z0-9._-]` + 拒绝纯点组合，防 `/nodeget/worker-route/{name}/*` 路径遍历。任何放宽都会直接打开 HTTP 路由的路径遍历面。
- **`token_time_valid` fail-closed**（`auth.rs:31`）：时间读取失败返回 `false`。列表过滤路径依赖它补齐时间校验，切勿移除。
- **`list_all_js_worker` 与 `get_rt_pool` 权限不同**：前者按 worker 逐个过滤，后者是 `Global` + `NodeGet::GetRtPool`（全局特权）。

## 依赖关系

- **依赖**：`ng-core`（错误、权限数据结构、`permission_checker` trait、时间戳工具）、`ng-db`（`js_worker`/`js_result` 实体、`get_db`）、`ng-infra`（`rpc_exec!`、`RpcHelper`、`token_identity`）、`ng-js-runtime`（运行时池、`js_runner`、字节码编译、`RunType`/`CompileMode`/`RuntimeLimits`）、`ng-token`（`check_super_token`、`get_token`、`check_limits_cover`，用于列表过滤快速路径）。
- **被依赖**：server binary（合并 `rpc_module()`；在 `serve.rs` 注册 `JsWorkerService`/`JsWorkerScheduler` 实现并消费内联调用服务）；`ng-crontab` 通过 trait `JsWorkerScheduler` 间接调度 `enqueue_defined_js_worker_run`。
- Worker 类型本身（`js_worker::Model`）由 `ng-db` 定义，本 crate 不重复定义。
