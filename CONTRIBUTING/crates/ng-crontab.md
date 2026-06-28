# ng-crontab

> 概览：ng-crontab 实现 NodeGet 的定时任务（cron）子系统。default feature 仅暴露共享数据类型（`Cron`、`CronType`、`AgentCronType`、`ServerCronType`、`CrontabResult`）以及 `CrontabResult` 的查询 DSL，使 agent 侧或其他轻量消费者可安全依赖而不引入 DB/RPC 代码；`server` feature 额外提供：(1) 全表 DB-backed 内存缓存，(2) 按最近触发 deadline 唤醒、单次 sleep 最长 60s 的调度器循环，(3) `crontab` 与 `crontab-result` 两个 JSON-RPC 命名空间。JS 执行通过 OnceLock 注入的 `JsWorkerScheduler` trait 与 ng-js-worker 解耦，权限校验通过 ng-core 的 `PermissionChecker` 解耦。

## 模块结构

```
crates/ng-crontab/src/
├── lib.rs                       # Crate root：声明模块、re-export 核心类型与 rpc_module()
├── cron_type.rs                 # 共享核心类型：Cron / CronType / AgentCronType / ServerCronType
├── result.rs                    # CrontabResult 数据结构（DB 行的 Rust 镜像）
├── query.rs                     # CrontabResult 查询 DSL
├── cache.rs                     # server：DB-backed 全表缓存（DbBackedCache + make_global_cache!）
├── server_cron.rs               # server：调度器循环 + delete/enable 辅助函数 + notify
├── task.rs                      # server：Agent 类型 cron 的任务下发 + JsWorkerScheduler 注入
└── rpc/
    ├── crontab/
    │   ├── mod.rs               # crontab 命名空间 trait + CrontabRpcImpl + rpc_module()
    │   ├── auth.rs              # CronType -> Scope/Permission 映射 + 权限校验
    │   ├── create.rs            # crontab.create 实现
    │   ├── edit.rs              # crontab.edit 实现
    │   ├── get.rs               # crontab.get 实现（按 scope 过滤）
    │   ├── delete.rs            # crontab.delete 实现
    │   └── set_enable.rs        # crontab.set_enable 实现
    └── crontab_result/
        ├── mod.rs               # crontab-result 命名空间 trait + Impl + rpc_module()
        ├── auth.rs              # CrontabResult 读/删权限校验（Global-only）
        ├── query.rs             # crontab-result.query 实现
        └── delete.rs            # crontab-result.delete 实现
```

## 公共 API

| 名称 | 签名 | 行为 |
|------|------|------|
| `Cron` | `pub struct Cron { id: i64, name: String, enable: bool, cron_expression: String, cron_type: CronType, last_run_time: Option<i64> }` | DB 行镜像；`Debug+PartialEq+Eq+Serialize+Deserialize`，snake_case。`last_run_time` 为 `None` 表示从未运行。 |
| `CronType` | `pub enum CronType { Agent(Vec<Uuid>, AgentCronType), Server(ServerCronType) }` | `Agent` = 下发至所列 agent UUID；`Server` = 在服务器本地执行。snake_case（`agent`/`server`）。 |
| `AgentCronType` | `pub enum AgentCronType { Task(TaskEventType) }` | 单变体，携带要下发的 `TaskEventType`。 |
| `ServerCronType` | `pub enum ServerCronType { JsWorker(String, serde_json::Value) }` | `(script_name, params)`；script_name 在写权限校验时必须非空（`auth.rs:129`）。 |
| `CrontabResult` | `pub struct CrontabResult { id, cron_id, cron_name, relative_id: Option<i64>, run_time: Option<i64>, success: Option<bool>, message: Option<String> }` | DB 行镜像；**不** `Serialize`。注意 query RPC 直接返回 SeaORM Model 而非此结构（见 query.rs:122）。 |
| `CrontabResultQueryCondition` | `pub enum CrontabResultQueryCondition { Id(i64), CronId(i64), CronName(String), RunTimeFromTo(i64,i64), RunTimeFrom(i64), RunTimeTo(i64), IsSuccess, IsFailure, Limit(u64), Last }` | 查询 DSL，条件 AND 组合；`Limit` 上限 10000，无 `Limit`/`Last` 时默认 1000。 |
| `CrontabResultDataQuery` | `pub struct CrontabResultDataQuery { condition: Vec<CrontabResultQueryCondition> }` | RPC 请求体。 |
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<()>`（server） | 合并 `crontab` 与 `crontab-result` 两个命名空间；任一合并失败会 `.expect` panic。 |
| `CrontabCache::init / global / reload` | `init() -> anyhow::Result<()>`；`global() -> Option<&'static CrontabCache>`；`reload() -> anyhow::Result<()>` | `make_global_cache!` 生成；`init` 从 DB 全量加载并注册单例，`global` 返回单例引用；`reload` 仅在已初始化时重读全表，未初始化时是 no-op。 |
| `CrontabCache::upsert / remove / remove_by_name` | `upsert(&self, m)`；`remove(&self, id)`；`remove_by_name(&self, name)` | 增量更新（按 id/按名）。 |
| `CrontabCache::get_enabled_entries / get_all_entries` | `-> Vec<Arc<CachedCrontab>>` | 前者仅 `enable==true`（调度器用），后者含禁用项（`crontab.get` RPC 用）。 |
| `CrontabCache::get_last_run_time / update_last_run_time` | `(id, model_last) -> Option<i64>`；`(id, ts)` | 读：优先 override map，回退 `model_last`；写：仅写 override map。 |
| `init_crontab_worker` | `pub fn init_crontab_worker()`（server） | OnceLock 守卫，仅 spawn 一次调度器任务；幂等。 |
| `notify_crontab_changed` | `pub fn notify_crontab_changed()`（server） | 提前唤醒调度器重算 deadline。 |
| `delete_crontab_by_name` | `pub async fn delete_crontab_by_name(name: String) -> Result<bool, DbErr>`（server） | 按名删除，返回是否删除到行；增量更新缓存 + 通知调度器。 |
| `set_crontab_enable_by_name` | `pub async fn set_crontab_enable_by_name(name: String, enable: bool) -> Result<Option<bool>, DbErr>`（server） | 返回 `Some(new_enable)` 或未找到时 `None`。 |
| `set_js_worker_scheduler` | `pub fn set_js_worker_scheduler(scheduler: Arc<dyn JsWorkerScheduler>)`（server） | 启动期注入器；OnceLock，第二次调用静默忽略。 |
| `js_worker_scheduler` | `pub fn js_worker_scheduler() -> Option<&'static Arc<dyn JsWorkerScheduler>>`（server） | 注入器访问器。 |

## 关键类型与常量

### `cron_type.rs`

- `Cron` (`cron_type.rs:13`)：字段含 `id`（i64 DB PK）、`name`（全局唯一）、`enable`、`cron_expression`（如 `*/5 * * * *`）、`cron_type: CronType`、`last_run_time: Option<i64>`（毫秒；`None` = 从未运行）。尽管含 `String`/`CronType`，仍 derive `Eq`（`CronType` 亦 derive `Eq`）。
- `CronType` (`cron_type.rs:31`)：`Agent(Vec<Uuid>, AgentCronType)` / `Server(ServerCronType)`。`Clone+Eq`，snake_case。
- `AgentCronType` (`cron_type.rs:41`)：`Task(TaskEventType)`，snake_case。
- `ServerCronType` (`cron_type.rs:49`)：`JsWorker(String /*script name*/, Value /*params*/)`；空 script name 在权限校验时被拒（`auth.rs:129`）。

### `cache.rs`

- `CachedCrontab` (`cache.rs:19`)：`{ model: crontab::Model, schedule: cron::Schedule, cron_type: CronType }`；由 cache 包在 `Arc` 中。**注意 `model.cron_type` 被 `mem::take` 掏空**，读取 cron_type 必须用 `CachedCrontab.cron_type`。
- `CrontabCache` (`cache.rs:40`)：`inner: RwLock<CrontabCacheInner{ by_id: HashMap<i64, Arc<CachedCrontab>> }>`，`last_run_times: RwLock<HashMap<i64, i64>>`（override map，优先于 `model.last_run_time`）。两把独立锁，使 last_run_time 更新不阻塞 by_id 读。
- `recover_read/recover_write` (`cache.rs:50`)： poison 时 `e.into_inner()` 恢复 + warning log，**绝不 panic**。
- `parse_one` (`cache.rs:118`)：解析单个 `Model` 为 `CachedCrontab`，用 `std::mem::take` 取走 `model.cron_type` 避免克隆；解析失败返回 `None` + warning（skip 语义）。
- `upsert` (`cache.rs:162`) / `remove` (`cache.rs:180`) / `remove_by_name` (`cache.rs:193`) / `get_enabled_entries` (`cache.rs:209`) / `get_all_entries` (`cache.rs:220`) / `get_last_run_time` (`cache.rs:231`) / `update_last_run_time` (`cache.rs:242`)。
- `make_global_cache!(CrontabCache, CRONTAB_CACHE_GLOBAL)` (`cache.rs:66`)：展开为 `static CRONTAB_CACHE_GLOBAL: OnceLock<CrontabCache>` + `init`/`global`/`reload`；`reload()` 在未初始化时直接返回 `Ok(())`。
- `impl DbBackedCache for CrontabCache` (`cache.rs:68`)：`type Model = crontab::Model`；`cache_name() -> "crontab"`；`reload_from_models` 替换 `inner.by_id` 但**保留** `last_run_times`。

### `query.rs`

- `CrontabResultQueryCondition` (`query.rs:12`)：snake_case；条件 AND 组合。
- `CrontabResultDataQuery` (`query.rs:37`)：`{ condition: Vec<...> }`。
- `CrontabResultResponseItem` (`query.rs:44`)：**仅** `Serialize`；**不**用于 RPC 响应（见陷阱）。

### `server_cron.rs`

- `CRONTAB_RELOAD_NOTIFY` (`server_cron.rs:24`)：`OnceLock<Arc<Notify>>`；`notify_one()` 在配置变更后提前唤醒调度器。
- `CRONTAB_WORKER_STARTED` (`server_cron.rs:111`)：`OnceLock<()>`，单次 spawn 守卫。
- `compute_next_deadline` (`server_cron.rs:147`)：遍历 enabled 项取 min next-after-last_run；**最多 cap 60s**；无 cache/无 job 返回 `now + cap`。
- `process_crontab` (`server_cron.rs:186`)：Phase 1 收集 due job；Phase 2 批量 `UPDATE crontab SET last_run_time=now WHERE id IN (...)` + per-id `cache.update_last_run_time`；Phase 3 `JoinSet` 并发跑 `run_job_logic`，panic 仅 log。
- `run_job_logic` (`server_cron.rs:307`)：按 `cron_type` 分发 —— `Agent` => `task::crontab_task`；`Server JsWorker` => `run_js_worker_job`。
- `run_js_worker_job` (`server_cron.rs:337`)：要求已注入 `js_worker_scheduler()`；调用 `enqueue_run(name, RunType::Cron, params, None)`；构建并插入 `CrontabResult` ActiveModel。

### `task.rs`

- `JsWorkerScheduler` trait (`task.rs:27`)：`Send+Sync+'static`；`enqueue_run(&self, worker_name, run_type, params, env_override) -> Pin<Box<dyn Future<Output=anyhow::Result<i64>>+Send>>`；返回 `relative_id`（JS worker run id）。
- `JS_WORKER_SCHEDULER` (`task.rs:45`)：`static OnceLock<Arc<dyn JsWorkerScheduler>>`。
- `crontab_task` (`task.rs:72`)：Phase 1 序列化 `task_event_type` 一次；Phase 2 构建 `Vec<task::ActiveModel>`（per-agent 随机 token `generate_random_string(10)`，`cron_source=Some(cron_name)`）；Phase 3 `insert_many`，`base_id = last_insert_id - (count-1)`；Phase 4 `JoinSet` 并发 `TaskManager::global().send_event` 发送 `TaskEvent{ task_id (u64 cast), task_token, task_event_type }`；Phase 5 收集结果，失败入 `failed_task_ids` 批量 `delete_many`（`Id.is_in`），构建 `CrontabResult`；Phase 6 `insert_many crontab_result`（单轮）；`info_span!` target `crontab`。

## 内部机制

### 调度器生命周期与唤醒节奏

`init_crontab_worker` (`server_cron.rs:119`) 通过 `CRONTAB_WORKER_STARTED` OnceLock 守卫 spawn 单个 tokio 任务。循环：`process_crontab()` -> `compute_next_deadline()` -> `tokio::select!` 在 `sleep_until(deadline)` 与 `reload_notify().notified()` 之间竞争。`compute_next_deadline` (`server_cron.rs:147`) 遍历 enabled 项取最近 `next-after-last_run`，按最早 deadline 休眠，**单次 sleep 最多 60s**，即使无 job 或下次触发很远也能周期性自检。

### Due-job 批处理避免 N 次往返

`process_crontab` (`server_cron.rs:186`)：Phase 1 收集 due job；Phase 2 单条 `UPDATE crontab SET last_run_time=now WHERE id IN (...)` + 廉价的 per-id override map 写；Phase 3 在 `JoinSet` 中并发执行 `run_job_logic`，`JoinError`（panic）仅 log。

### 双锁缓存避免竞争

`CrontabCache` 将 `last_run_times` 放在与 `inner.by_id` 独立的 `RwLock` 中（`cache.rs:46`）。`get_last_run_time`/`update_last_run_time` 只触碰 `last_run_times`，调度器时间戳更新永不阻塞 `crontab.get` 读 by_id。两把锁均走 `recover_read/recover_write`（`cache.rs:50/58`），poison 时 warning 恢复而非 panic。

### 增量缓存更新 vs 全量 reload

写操作（create/edit/delete/set_enable 及 server_cron 辅助函数）优先使用 `cache.upsert/remove/remove_by_name` + `notify_crontab_changed()`；仅当 `CrontabCache::global()` 为 `None` 时才会尝试 `CrontabCache::reload().await`，但该 `reload()` 在 cache 尚未初始化时是 no-op。`parse_one` (`cache.rs:118`) 用 `std::mem::take` 避免 `Value` 克隆。

### last_run_times 跨 reload 存活

`reload_from_models` (`cache.rs:88`) 故意保留 override map，使运行期 last_run 跟踪跨热重载连续。

### crontab_task ID 派生依赖连续自增

`task.rs:159` 计算 `base_id = last_insert_id - (agent_count-1)`，假设 DB 为 `insert_many` 批次分配连续自增 id。SQLite/Postgres 串行自增下正确；若 id 非连续（序列间隙、trigger、副本）则 task id 与下发给 agent 的 `TaskEvent.task_id` 将出错。per-agent `task_id = base_id + i`。

### 配置变更通知

`CRONTAB_RELOAD_NOTIFY` (`server_cron.rs:24`) 经 `reload_notify()` 惰性初始化。`notify_crontab_changed()` 调 `notify_one()`，唤醒调度器 `select!` 臂，使其在 create/edit/delete/set_enable 后立即重算 deadline。

### JS 执行的 trait 注入

`JsWorkerScheduler` (`task.rs:27`) 存于 `static OnceLock<Arc<dyn ...>>`；服务器启动时注册具体实现。Server-cron JS 任务（`run_js_worker_job`，`server_cron.rs:337`）调 `enqueue_run(name, RunType::Cron, params, None)` 返回 `relative_id`；若 scheduler 未注入，记一条 failure `CrontabResult`。

### Cache 通过 mem::take 预解析 cron_type

`parse_one` (`cache.rs:118`) 执行 `serde_json::from_value::<CronType>(std::mem::take(&mut model.cron_type))`；之后 `CachedCrontab.model.cron_type` 为 null。**读缓存项必须用 `CachedCrontab.cron_type`，绝不用 `CachedCrontab.model.cron_type`**。

### crontab-result query 直接返回 SeaORM Model

`crontab_result.query` (`query.rs:122`) 经 `serde_json::to_raw_value` 直接序列化 `Vec<crontab_result::Model>`。`query.rs::CrontabResultResponseItem` 与 `result.rs::CrontabResult` **不**用于 RPC 响应——它们是文档/占位类型。响应字段名来自 SeaORM Model（entity 生成时 `serde both`）。

### Tracing target 与 span

领域日志主要使用 `"crontab"` 与 `"crontab_result"`；`cache.rs` 的锁 poison 恢复用 `"crontab_cache"`，但同模块的解析失败告警仍记到 `"crontab"`。另外，`make_global_cache!` 与 `rpc_exec!` 带来的基础设施日志分别落在 `"cache"` 与 `"rpc"`。每个 `RpcServer` 方法开 `info_span!`（如 `crontab::create`、`crontab-result::query`），携带 `token_key`、`username`、`name` 等。`crontab_task` 与 `run_job_logic` 有自己的嵌套 span（`crontab::dispatch_task`、`crontab::run_job`）。

## RPC 方法

### `crontab` 命名空间

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `create` | `token, name, cron_expression, cron_type` | `ensure_crontab_payload_write_permission` —— 所有派生 scope 上的 `Crontab::Write`；Agent 另需 `Task::Create(<task_name>)`；Server 另需 `JsWorker::RunDefinedJsWorker`（scope `JsWorker(worker_name)`） | 校验 name、token、写权限、cron 表达式（`Schedule::from_str`）、名称唯一性；插入 `enable=true, last_run_time=None`；`cache.upsert` + notify。返回 `{"id": <new id>}`。 |
| `edit` | `token, name, cron_expression, cron_type` | 原 cron_type scope 上的 `ensure_crontab_scope_permission(Crontab::Write)` **加** 新 cron_type 的 `ensure_crontab_payload_write_permission` | 按名查（未找到 NotFound）、校验新 cron 表达式、仅更新 `cron_expression`+`cron_type`；`cache.upsert` + notify。返回 `{"id": id, "success": true}`。 |
| `get` | `token` | Super-token 旁路；否则至少一个 limit 含 `Crontab::Read`，按 Global/AgentUuid scope 过滤 | Super-token 返回全部；校验时间戳；`filter_entries_by_token`：Global=全部，AgentUuid=该 uuid 命中项可见，Server=仅 Global。返回 `Vec<Cron>` JSON。 |
| `delete` | `token, name` | `ensure_crontab_scope_permission(Crontab::Delete)`（空 scope => Global） | 按名查、解析 cron_type、权限校验、`delete_crontab_by_name`。返回 `{"success": <bool>}`。双重 404 检查。 |
| `set_enable` | `token, name, enable` | `ensure_crontab_scope_permission(Crontab::Write)` | 按名查、权限校验、`set_crontab_enable_by_name`。返回 `{"success": true, "enabled": <actual>}`。 |

### `crontab-result` 命名空间

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `query` | `token, query: CrontabResultDataQuery` | `check_crontab_result_read_permission` —— Global scope；`CrontabResult::Read("*")` 或 `Read(<each cron_name>)` | 应用过滤；若含 CronName 则逐名校验，否则校验全局 `"*"`；无 `Last` 时 `ORDER BY run_time DESC`；无显式 `Limit`/`Last` 时默认 `LIMIT 1000`。返回序列化 `Vec<crontab_result::Model>`。 |
| `delete` | `token, query: CrontabResultDataQuery` | `check_crontab_result_delete_permission` —— Global scope；`CrontabResult::Delete("*")` 或 `Delete(<each cron_name>)`（`None` => 仅全局） | 收集 distinct CronName 做权限校验；构建两条并行查询（select_only Id + delete_many），过滤相同（`Limit` 上限 10000，`Last` 置标志）；若 `is_last || limit_count.is_some()`：select ids `ORDER BY run_time DESC, id DESC LIMIT n` 后 `delete_many WHERE Id IN`；否则直接 `delete_many`。返回 `{"success": true, "deleted": rows, "condition_count": n}`。 |

### 鉴权流程

`crontab` 命名空间分两类路径：
- `create/edit/delete/set_enable` 走 `TokenOrAuth::from_full_token` -> `require_permission_checker()` -> `check_token_limit(...)`。`scopes_from_cron_type`（`auth.rs:22`）将 `Agent` 映射为 per-uuid 的 `AgentUuid`（HashSet 去重）、`Server` 映射为 `[Global]`；`write_permissions_from_cron_type`（`auth.rs:38`）恒含 `[Crontab::Write]`，Agent Task 另加 `Task::Create(task_name)`。
- `get` 是例外：先 `TokenOrAuth::from_full_token`，再直接调用 `ng_token::check_super_token` / `ng_token::get_token`，手动校验时间窗与是否存在任一 `Crontab::Read` limit，然后按 Global/AgentUuid scope 过滤可见条目；它**不**经过 `PermissionChecker` 注入路径。

`crontab-result` 命名空间：仅 Global scope；先查通配 `*`，再查具体 `cron_name`；delete 时 `cron_name=None` 仅校验通配。

## 数据库实体 / 迁移

| 表名 | 列 | 约束/索引/关系 | 备注 |
|------|----|-----------------|------|
| `crontab` | `id`（i64 自增 PK）、`name`（text，唯一）、`cron_expression`（text）、`cron_type`（JSON Value，序列化 `CronType`）、`enable`（bool）、`last_run_time`（nullable i64 毫秒） | `id` 为 PK；`name` 在 entity 与迁移层都强制唯一（列定义 `unique_key`，另有唯一索引 `idx-crontab-name`）；由 ng-db entity crate 拥有，ng-crontab 经 SeaORM ActiveModel 读写 | create/edit 校验 cron 表达式；缓存加载校验 cron_type JSON。`set_enable` 仅改 enable；`edit` 仅改 `cron_expression`+`cron_type`。注意 `idx-crontab-name` 目前与列唯一约束重复。 |
| `crontab_result` | `id`（i64 自增 PK）、`cron_id`（i64 逻辑 FK -> crontab.id）、`cron_name`（text，结果创建时 crontab.name 的去规范化快照）、`relative_id`（nullable i64；Agent task id 或 JS Worker run id）、`run_time`（nullable i64 毫秒）、`success`（nullable bool）、`message`（nullable text） | `id` 为 PK；迁移中另建了冗余唯一索引 `idx-crontab_result-id`；普通索引含 `idx-crontab_result-relative_id`、`idx-crontab_result-cron_id`、`idx-crontab_result-cron_name`，以及 `idx-crontab_result-run_time` DESC；无强制 FK；无 soft-delete，`delete_many` 物理删除 | 经动态 SeaORM filter 链读写；`message` 含中文。 |

迁移由 `crates/ng-db/migration/` 拥有，ng-crontab 本身不定义迁移步骤。

## Crate 内部约定

- **Feature 门控**：`default = []` 仅暴露类型（`Cron`、`CronType`、`AgentCronType`、`ServerCronType`、`CrontabResult`、query DSL）；`server` feature 引入 cache/rpc/server_cron/task 模块。
- **Serde**：`cron_type.rs` 中的共享类型与 `query.rs` 里的 `CrontabResultQueryCondition` 使用 `#[serde(rename_all = "snake_case")]`；`CrontabResultDataQuery` / `CrontabResultResponseItem` 没有该属性。
- **Logging target**：领域日志主要见于 `crontab` 与 `crontab_result`；`crontab_cache` 仅用于锁 poison 恢复；基础设施层还会出现 `cache`（全局缓存 init/reload）与 `rpc`（`rpc_exec!`）target。
- **中文**：注释与日志/result 消息（如 `'任务下发成功'`、`'定时任务_1'`）含中文；`crontab_result.message` 嵌入中文。
- **RPC 风格**：所有方法返回 `RpcResult<Box<RawValue>>`；一律 `#[rpc(server, namespace = "...")]` + `#[method(name = "...")]`；业务逻辑包在 `rpc_exec!`（来自 ng-infra）。
- **jsonrpsee 分隔符**：自定义 fork 以 `_` 为命名空间分隔符，故 `crontab-result` 命名空间方法被调用为 `crontab-result_query` 等。
- **错误处理**：业务逻辑返回 `anyhow::Result`/`Result<_, NodegetError>`；外层经 `anyhow_to_nodeget_error` 映射为 `ErrorObject::owned(code, msg, None::<()>)`。
- **权限**：`crontab` 的 create/edit/delete/set_enable 走 `TokenOrAuth::from_full_token` + `PermissionChecker::check_token_limit(...)`；`get` 则直接用 `ng_token::check_super_token/get_token` 做时间窗与 limit/scope 过滤。`CrontabResult` 相关权限仅 Global scope，通配 `*` 或具体 cron_name。
- **缓存写纪律**：写操作（create/edit/delete/set_enable）调 `cache.upsert/remove/remove_by_name` + 若已初始化则 `notify_crontab_changed()`；否则回退 `CrontabCache::reload().await`。
- **DB 访问**：一律经 `ng_db::get_db() -> Option<&'static DatabaseConnection>`；`None` 时映射为合成 `DbErr::Conn(Internal)` 或 `NodegetError::DatabaseError`。
- **Edition 2024**：使用 `if let ... && let ...` 链、let-else、`.is_some_and`。

## 注意事项与陷阱

- **维护者必须**用 `CachedCrontab.cron_type` 读 cron_type，**切勿**读 `CachedCrontab.model.cron_type`——后者在 `parse_one` 中被 `std::mem::take` 掏空为 null（`crates/ng-crontab/src/cache.rs:137`）。
- **切勿**假设 `insert_many` 在非串行后端返回连续 id：`crontab_task` 依赖 `base_id = last_insert_id - (count-1)`，非连续 id（序列间隙、trigger、副本）会使下发的 `task_id` 错误（`crates/ng-crontab/src/task.rs:159`）。
- **注意** `process_crontab` 在 spawn 执行**之前**批量更新所有 due job 的 `last_run_time`；若 job panic 或在 UPDATE 与完成之间进程崩溃，该次触发被标记为已运行但无 `CrontabResult`，**静默丢失**，无回滚（`crates/ng-crontab/src/server_cron.rs:186`）。
- **注意** 若 `last_run_time` 批量 UPDATE 失败（DB error），代码只 log、不更新 override map，仍照常 spawn job；这会导致每个 tick 重复触发（`schedule.after(last_run).next()` 持续返回过去时间），形成重复下发风暴（`crates/ng-crontab/src/server_cron.rs:242`）。
- **注意** Agent 类型 cron 在调度层是 fire-and-forget：`crontab_task` 仅依据 `send_event`（channel 入队）是否成功记录 success，不反映 agent 是否真正执行；`CrontabResult.success` 含义是「已入队」而非「已运行」（`crates/ng-crontab/src/server_cron.rs:318`）。
- **注意** `init_crontab_worker` 无关闭句柄：`CRONTAB_WORKER_STARTED` 是不可重置的 `OnceLock<()>`，spawn 的任务带无 cancellation token 地永久循环；除结束进程外无法停止调度器（`crates/ng-crontab/src/server_cron.rs:128`）。
- **注意** 文档中的 `CrontabResultResponseItem`（`query.rs:44`）与 `result.rs::CrontabResult` **并非** `crontab-result.query` 的返回类型；实际响应是 SeaORM `crontab_result::Model` 的裸序列化，字段名以 entity 序列化为准（`crates/ng-crontab/src/rpc/crontab_result/query.rs:122`）。
- **注意** 空 UUID 列表的 Agent cron 写权限降级为仅 Global `Crontab::Write`（无 `Task::Create`），等同空操作下发但权限语义更弱；改动 `Task::Create` 校验须记得此空列表特例（`crates/ng-crontab/src/rpc/crontab/auth.rs:93`）。
- **注意** `edit` 同时对**原** cron_type scope 与**新** cron_type scope 做写权限校验；用户编辑一个已无权限的 agent 目标 cron 会被拒（即使新配置不指向那些 agent）——这是设计行为，易被误诊为权限 bug（`crates/ng-crontab/src/rpc/crontab/edit.rs:57`）。
- **注意** `delete` 存在冗余双重查找：先 find 做权限校验，`delete_crontab_by_name` 又自行 `delete_many WHERE name=`；两步间的 race 可使第二步删 0 行并返回 NotFound。`set_enable` 的 find+update 同样非原子（`crates/ng-crontab/src/rpc/crontab/delete.rs:63`）。
- **注意** `validate_name` 仅在 RPC 层（create/edit/delete/set_enable）应用；底层 `delete_crontab_by_name`/`set_crontab_enable_by_name` **不**校验 name，直接传给 SQL filter。直接内部调用者可能注入含 `/` 或控制字符的 name——路径安全不变量依赖始终经 RPC（`crates/ng-crontab/src/server_cron.rs:41`）。
- **注意** `crontab.get` 中，仅有 AgentUuid scope 的 `Crontab::Read`（无 Global）的 token 看不到任何 Server 类型 job（`filter_entries_by_token` 在 `!has_global` 时对 Server 项返回 false）；无 Global scope 的 token 持有者会发现 Server cron 完全不可见（`crates/ng-crontab/src/rpc/crontab/get.rs:83`）。
- **注意** `cron_name=None` 的 CrontabResult delete 仅校验通配 `Delete("*")`；无 CronName 过滤的查询若不具备全局通配删除权限即被拒——无 fallback 到具体名权限（`crates/ng-crontab/src/rpc/crontab_result/auth.rs:119`）。

## 依赖关系

ng-crontab 依赖工作区内的 ng-core（`PermissionChecker`/`Permission`/`Scope`、`NodegetError`、`TokenOrAuth`、`Token`、`get_local_timestamp_ms_i64`、`generate_random_string`、`anyhow_to_nodeget_error`）、ng-db（`crontab`/`crontab_result`/`task` entity、`get_db`、`DbErr`）、ng-infra（`DbBackedCache`、`make_global_cache!`、`rpc_exec!`、`RpcHelper`、`token_identity`）、ng-task（`TaskEventType`、`TaskManager`、`task::ActiveModel`、`TaskEvent`）。`Cron`/`CronType`/`AgentCronType`/`ServerCronType` 由 ng-crontab 自身定义。Cron 表达式解析依赖外部 `cron` crate；UUID 依赖 `uuid`。default feature 对 agent 侧消费者是安全的，但当前 `nodeget-agent` crate 实际并**不**依赖 ng-crontab。服务器二进制启用 `server` feature 并在启动时调用 `init_crontab_worker()`、`CrontabCache::init()`、`set_js_worker_scheduler`，并把 `rpc_module()` 合并入主模块；`JsWorkerScheduler` trait 定义在 ng-crontab，而具体的 `CronJsWorkerScheduler` 实现在 `server/src/subcommands/serve.rs` 中。
