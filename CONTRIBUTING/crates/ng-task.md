# ng-task

> 概览：ng-task 是 NodeGet 的任务调度 RPC crate。默认（types-only）配置下提供 agent 安全的数据模型（`TaskEventType` / `TaskEvent` / `TaskEventResult` / `TaskEventResponse` 及各任务类型的参数/结果结构体和查询 DSL）；开启 `server` feature 后暴露 `task` JSON-RPC 命名空间（`register_task` / `create_task` / `create_task_blocking` / `upload_task_result` / `query` / `delete`）、`TaskManager` 广播中枢（按 agent 维护 WebSocket 会话 + oneshot 阻塞等待表），以及通过 OnceLock 注入的 `MonitoringUuidProvider` trait（用于在创建任务时登记目标 agent UUID）。

## 模块结构

```
crates/ng-task/src/
├── lib.rs                       # crate 根，default 暴露 types，server 暴露 rpc
├── types/
│   ├── mod.rs                   # 任务数据结构（agent 安全，无 feature 门控）
│   └── query.rs                 # 查询 DSL：TaskQueryCondition / TaskDataQuery / TaskResponseItem
└── rpc/                         # #[cfg(feature = "server")]
    ├── mod.rs                   # Rpc trait、TaskRpcImpl、TaskManager、MonitoringUuidProvider、register_task、escape_like_pattern、rpc_module
    ├── create_task.rs           # 非阻塞创建任务
    ├── create_task_blocking.rs  # 阻塞创建任务（等待 agent 上传结果）
    ├── upload_task_result.rs    # agent 上传任务结果
    ├── query.rs                 # 任务查询
    └── delete.rs                # 任务删除
```

| 文件 | 角色 |
|---|---|
| `lib.rs:15` | `pub mod types;` + `pub use types::query;`，并在 crate 根 glob 重导出核心结构体/枚举 |
| `lib.rs:27` | `#[cfg(feature = "server")] pub mod rpc;`，重导出 `MonitoringUuidProvider / TaskManager / monitoring_uuid_provider / rpc_module / set_monitoring_uuid_provider` 这五个服务端公共名 |
| `types/mod.rs` | 全部任务数据结构，agent 与 server 共享 |
| `types/query.rs` | 查询/删除用的条件 DSL |
| `rpc/mod.rs` | `#[rpc(server, namespace = "task")]` trait、`TaskRpcImpl`、`TaskManager`（全局 OnceLock 单例 + peer 表 + 阻塞等待表）、`MonitoringUuidProvider` trait + 注入、`register_task` 订阅 handler、`escape_like_pattern` 辅助、`rpc_module()` 构造器 |
| `rpc/create_task.rs` | 非阻塞创建：校验、授权 `Task::Create`、插入 DB 行、登记 monitoring_uuid、`send_event`、失败回滚 |
| `rpc/create_task_blocking.rs` | 阻塞创建：在 `send_event` 之前注册 oneshot 等待者，带超时等待 agent 上传的结果 |
| `rpc/upload_task_result.rs` | 两阶段权限校验、`(task_id, agent_uuid, task_token)` 三元组校验、拒绝重复上传、更新行、唤醒阻塞等待者 |
| `rpc/query.rs` | 授权 `Task::Read`，按条件构造查询，流式序列化为 JSON 数组（默认上限 1000） |
| `rpc/delete.rs` | 授权 `Task::Delete`，按条件删除（Last/Limit 走两步：先选 ID 再按 ID 删除） |

## 公共 API

| 名称 | 签名 | 行为 |
|---|---|---|
| `TaskEventType` 等 types | `pub enum TaskEventType { Ping(String), TcpPing(String), HttpPing(url::Url), WebShell(WebShellTask), Execute(ExecuteTask), HttpRequest(HttpRequestTask), Dns(DnsTask), ReadConfig, EditConfig(String), Ip, Version, SelfUpdate(String) }` | 始终可用。任务数据模型。`TaskEvent.task_id` 在线路上是 `u64`，DB 中是 `i64`。附带 `const fn task_name`、`fn result_from_duration`、`const fn is_ping_task`、`const fn permission_field` |
| `TaskDataQuery` 等 | `pub struct TaskDataQuery { pub condition: Vec<TaskQueryCondition> }` | 始终可用。被 `query` 与 `delete` RPC 用作参数 |
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<TaskRpcImpl>`（server feature） | 构造交给 server `build_modules()` 的 `RpcModule`；内部使用全局 `TaskManager` 单例 |
| `TaskManager` | `#[derive(Clone)] pub struct TaskManager { ... }`，含 `new` / `global` / `add_session` / `remove_session` / `send_event` / `register_blocking_waiter` / `remove_blocking_waiter` / `notify_blocking_waiter` | 可 Clone 的句柄，持有 Arc 化的 peer 表与阻塞等待表。`global()` 返回进程级单例 |
| `MonitoringUuidProvider` | `pub trait MonitoringUuidProvider: Send + Sync + 'static`，方法 `get_or_insert(&self, uuid: Uuid) -> Future<Output=Result<i16, NodegetError>>`、`reload(&self) -> Future<Output=anyhow::Result<()>>` | 对象安全 trait，由 server binary 实现；ng-task 在创建任务时调用 `get_or_insert(target_uuid)` |
| `set_monitoring_uuid_provider` / `monitoring_uuid_provider` | `pub fn set_monitoring_uuid_provider(provider: Arc<dyn MonitoringUuidProvider>)`；`pub fn monitoring_uuid_provider() -> Option<&'static Arc<dyn MonitoringUuidProvider>>` | OnceLock 注入。`set_` 在 server 启动时调用一次；未设置时 `get` 返回 `None` |

## 关键类型与常量

### 任务类型枚举（`types/mod.rs`）

| 项 | 位置 | 说明 |
|---|---|---|
| `TaskEventType` | `crates/ng-task/src/types/mod.rs:118` | derive `Debug, PartialEq, Eq, Clone, Serialize, Deserialize`。变体：`Ping(String)`、`TcpPing(String)`、`HttpPing(url::Url)`、`WebShell(WebShellTask)`、`Execute(ExecuteTask)`、`HttpRequest(HttpRequestTask)`、`Dns(DnsTask)`、`ReadConfig`、`EditConfig(String)`、`Ip`、`Version`、`SelfUpdate(String)`。各载荷结构体同样 derive 这四个 trait（无自定义 `Default`） |
| `TaskEventType::task_name` | `crates/ng-task/src/types/mod.rs:153` | `const fn task_name(&self) -> &'static str`，将变体映射为稳定的 snake_case 标识（`ping`、`tcp_ping`、`http_ping`、`web_shell`、`execute`、`http_request`、`dns`、`edit_config`、`read_config`、`ip`、`version`、`self_update`），用于权限校验与 DB JSON 键匹配 |
| `TaskEventType::result_from_duration` | `crates/ng-task/src/types/mod.rs:174` | `fn result_from_duration(&self, duration: Duration) -> Option<TaskEventResult>`，毫秒换算 `as_secs_f64()*1000.0`。仅 `Ping/TcpPing/HttpPing` 返回 `Some`，其余 `None` |
| `TaskEventType::is_ping_task` | `crates/ng-task/src/types/mod.rs:186` | `const fn is_ping_task(&self) -> bool`，仅 Ping 三类为真 |
| `TaskEventType::permission_field` | `crates/ng-task/src/types/mod.rs:193` | `const fn permission_field(&self) -> &'static str`，返回控制该任务的 Agent 配置 `allow_*` 字段名（如 `Ping->allow_icmp_ping`、`Execute->allow_execute`、`SelfUpdate->allow_self_update`） |

### 线路结构（`types/mod.rs`）

| 项 | 位置 | 说明 |
|---|---|---|
| `TaskEvent` | `crates/ng-task/src/types/mod.rs:213` | derive `Debug, PartialEq, Eq, Clone, Serialize, Deserialize`。字段：`task_id: u64`、`task_token: String`、`task_event_type: TaskEventType`。`task_id` 线路为 `u64`（DB 为 `i64`）。`task_token` 是用于认证结果上传者的随机 nonce，不用于创建授权 |
| `TaskEventResult` | `crates/ng-task/src/types/mod.rs:226` | derive `Debug, PartialEq, Clone, Serialize, Deserialize`，带 `#[serde(rename_all="snake_case")]` 与 `#[allow(clippy::large_enum_variant)]`。变体：`Ping(f64)`、`TcpPing(f64)`、`HttpPing(f64)`（毫秒）、`WebShell(bool)`、`Execute(String=stdout)`、`HttpRequest(HttpRequestTaskResult)`、`Dns(Vec<DnsRecordResult>)`、`ReadConfig(String)`、`EditConfig(bool)`、`Ip(Option<Ipv4Addr>, Option<Ipv6Addr>)`、`Version(NodeGetVersion)`、`SelfUpdate(bool)`。不含 `Eq`（含 `f64`） |
| `TaskEventResult::task_name` | `crates/ng-task/src/types/mod.rs:261` | `const fn task_name(&self) -> &'static str`，与 `TaskEventType::task_name` 镜像 |
| `TaskEventResult::from_duration` | `crates/ng-task/src/types/mod.rs:280` | `const fn from_duration(task_type: &TaskEventType, duration: Duration) -> Option<Self>` |
| `TaskEventResponse` | `crates/ng-task/src/types/mod.rs:292` | derive `Debug, PartialEq, Clone, Serialize, Deserialize`。字段：`task_id: u64`、`agent_uuid: uuid::Uuid`、`task_token: String`、`timestamp: u64`（毫秒）、`success: bool`、`error_message: Option<String>`、`task_event_result: Option<TaskEventResult>`。即 `upload_task_result` 校验的 agent 上传结果载荷 |

### 各任务类型参数/结果（`types/mod.rs`）

| 项 | 位置 | 说明 |
|---|---|---|
| `WebShellTask` | `crates/ng-task/src/types/mod.rs:20` | `{ url: url::Url, terminal_id: uuid::Uuid }` |
| `ExecuteTask` | `crates/ng-task/src/types/mod.rs:29` | `{ cmd: String, args: Vec<String> }`。`cmd` 必须非空（trim 后），由 `create_task.rs` 中的 `validate_task_type` 强制 |
| `HttpRequestTask` | `crates/ng-task/src/types/mod.rs:38` | `{ url, method: String, headers: BTreeMap<String,String> (#[serde(default)]), body: Option<String>, body_base64: Option<String>, ip: Option<String> }`。`body` 与 `body_base64` 互斥（约定，不在此强制） |
| `DnsRecordType` | `crates/ng-task/src/types/mod.rs:57` | serde `rename_all="snake_case"`：`A`、`Aaaa(->aaaa)`、`Cname`、`Mx`、`Txt`、`Ns`、`Srv`、`Ptr`、`Soa`、`Caa` |
| `DnsTask` | `crates/ng-task/src/types/mod.rs:82` | `{ domain: String, record_types: Vec<DnsRecordType>, dns_server: Option<String> }` |
| `DnsRecordResult` | `crates/ng-task/src/types/mod.rs:93` | `{ record_type: DnsRecordType, time: f64 (ms), data: String }`。不含 `Eq`（`f64`） |
| `HttpRequestTaskResult` | `crates/ng-task/src/types/mod.rs:104` | `{ status: u16, headers: Vec<BTreeMap<String,String>>（数组形式，允许重复键）, body: Option<String>, body_base64: Option<String> }` |

### 查询 DSL（`types/query.rs`）

| 项 | 位置 | 说明 |
|---|---|---|
| `TaskQueryCondition` | `crates/ng-task/src/types/query.rs:9` | derive `Debug, PartialEq, Eq, Serialize, Deserialize`，`#[serde(rename_all="snake_case")]`（外部标签新类型/按形状反序列化）。变体：`TaskId(u64)`、`Uuid(uuid::Uuid)`、`TimestampFromTo(i64,i64)`、`TimestampFrom(i64)`、`TimestampTo(i64)`、`IsSuccess`、`IsFailure`、`IsRunning`、`Type(String)`、`CronSource(String)`、`Limit(u64)`、`Last`。条件之间 AND 组合 |
| `TaskDataQuery` | `crates/ng-task/src/types/query.rs:41` | derive `Debug, PartialEq, Eq, Serialize, Deserialize`。字段 `condition: Vec<TaskQueryCondition>`。空 vec 合法（退化为 Global 作用域、全类型查询，受 `DEFAULT_LIMIT` 限制） |
| `TaskResponseItem` | `crates/ng-task/src/types/query.rs:48` | 仅 derive `Serialize`（非 `Deserialize`，只写响应）。字段：`task_id: i64`、`uuid: String`、`cron_source: Option<String>`、`timestamp: Option<i64>`、`success: Option<bool>`、`task_event_type: Value`、`task_event_result: Option<Value>`、`error_message: Option<String>`。**注意**：此结构体已定义但 query RPC 并不序列化进它——而是直接用重命名键流式输出 `RawValue` JSON。`task_id` 在此为 `i64`，与 `TaskEvent` 的 `u64` 不一致 |

### RPC 层类型（`rpc/mod.rs`）

| 项 | 位置 | 说明 |
|---|---|---|
| `escape_like_pattern` | `crates/ng-task/src/rpc/mod.rs:35` | `pub(crate) fn escape_like_pattern(pattern: &str) -> String`，转义反斜杠、`%`、`_`。在 SQLite 分支的 Type 条件 JSON 文本 LIKE 中使用 |
| `MonitoringUuidProvider` | `crates/ng-task/src/rpc/mod.rs:50` | `pub trait MonitoringUuidProvider: Send + Sync + 'static`，返回 boxed future 的两个方法：`get_or_insert`（确保目标 agent UUID 存在，返回 `i16` id）、`reload`（刷新缓存，ng-task 自身不使用）。由 server binary（`TaskMonitoringUuidProvider`）注入 |
| `MONITORING_UUID_PROVIDER` 等 | `crates/ng-task/src/rpc/mod.rs:64` | `static MONITORING_UUID_PROVIDER: OnceLock<Arc<dyn MonitoringUuidProvider>>`。`set_monitoring_uuid_provider` 调用 `.set()` 并忽略 `Result`（已设置则静默无操作）。`monitoring_uuid_provider() -> Option<&'static Arc<dyn MonitoringUuidProvider>>` 读取 |
| `Rpc` | `crates/ng-task/src/rpc/mod.rs:80` | `#[rpc(server, namespace = "task")] trait Rpc`，声明 1 个订阅 + 5 个方法，均返回 `RpcResult<Box<RawValue>>`（订阅返回 `SubscriptionResult`） |
| `TaskRpcImpl` | `crates/ng-task/src/rpc/mod.rs:131` | `pub struct TaskRpcImpl { pub manager: Arc<TaskManager> }`；`impl RpcHelper for TaskRpcImpl {}`；`#[async_trait] impl RpcServer` 手动接线：每个方法调用 `token_identity(&token)`、构建 `info_span!(target:"task",...)`、在 `.instrument(span)` 内执行 `rpc_exec!`。`register_task` 为手写 |
| `Peers` | `crates/ng-task/src/rpc/mod.rs:366` | `type Peers = Arc<RwLock<HashMap<Uuid, (Uuid, mpsc::Sender<TaskEvent>)>>>`——agent UUID ->（订阅 reg_id，task-event sender）。`reg_id`（每次订阅新生成 `Uuid::new_v4()`）是所有权令牌，用于防止陈旧的断开驱逐同 UUID 的更新会话 |
| `BlockingWaiters` | `crates/ng-task/src/rpc/mod.rs:370` | `type BlockingWaiters = Arc<std::sync::RwLock<HashMap<u64, oneshot::Sender<TaskEventResponse>>>>`——task_id -> oneshot sender。使用 `std::sync::RwLock`（非 tokio）因为临界区无 `.await`；`write().unwrap_or_else(|e| e.into_inner())` 从中毒恢复 |
| `TaskManager` | `crates/ng-task/src/rpc/mod.rs:382` | `#[derive(Clone)] pub struct TaskManager { peers: Peers, blocking_waiters: BlockingWaiters }`。Clone 廉价（Arc 内部）。`Default::default()` 委托给 `new()` |
| `TaskManager::global` / `GLOBAL_TASK_MANAGER` | `crates/ng-task/src/rpc/mod.rs:407` | `pub fn global() -> &'static Arc<Self>`，经 `GLOBAL_TASK_MANAGER: OnceLock<Arc<TaskManager>>` 的 `get_or_init`。`rpc_module()` 使用 `TaskManager::global().clone()` |
| `TaskManager::add_session` | `crates/ng-task/src/rpc/mod.rs:416` | `pub async fn add_session(&self, uuid, reg_id, tx)`，取写锁插入（覆盖该 UUID 的任何先前会话）。debug 级日志 |
| `TaskManager::remove_session` | `crates/ng-task/src/rpc/mod.rs:427` | `pub async fn remove_session(&self, uuid, reg_id)`，仅当存储的 reg_id 与传入一致时移除 |
| `TaskManager::send_event` | `crates/ng-task/src/rpc/mod.rs:445` | `pub async fn send_event(&self, uuid, event) -> Result<(), (i32, String)>`，读锁内克隆 Sender 后立即释放锁（避免写锁头阻塞），使用 `tx.try_send`（非 `send().await`）——满队列（容量 32）立即返回 `Err((104, "...task queue is full"))`；断开 -> `Err((104,...))`；未知 UUID -> `Err((104, "Agent ... is not connected"))` |
| `TaskManager::register_blocking_waiter` | `crates/ng-task/src/rpc/mod.rs:481` | `pub fn register_blocking_waiter(&self, task_id) -> oneshot::Receiver<TaskEventResponse>`。**必须**在 `send_event` 之前调用，否则 agent 可能在等待者存在前返回（竞态） |
| `TaskManager::remove_blocking_waiter` | `crates/ng-task/src/rpc/mod.rs:495` | 超时/发送失败/通道关闭时调用 |
| `TaskManager::notify_blocking_waiter` | `crates/ng-task/src/rpc/mod.rs:505` | 移除条目；若存在则发送响应（忽略发送错误）并返回 true，否则 false。由 `upload_task_result` 调用 |
| `rpc_module` | `crates/ng-task/src/rpc/mod.rs:524` | `pub fn rpc_module() -> jsonrpsee::RpcModule<TaskRpcImpl>`，构建 `TaskRpcImpl{manager: TaskManager::global().clone()}` 并调用 `.into_rpc()` |

### 常量

| 常量 | 值 | 说明 |
|---|---|---|
| `DEFAULT_LIMIT` | `1000` | query 在既无 `Last` 也无 `Limit` 条件时的默认上限（与 `crontab_result`/`js_result` 约定一致） |
| `MAX_LIMIT` | `10_000` | query/delete 的 `Limit` 上限（重复定义以镜像其他结果 crate） |
| `MAX_TIMEOUT_MS` | `300_000` | `create_task_blocking` 的超时上限（毫秒） |
| mpsc 容量 | `32` | `register_task` 为每个会话创建的 `mpsc(32)` 通道，`send_event` 据此 `try_send` |

## 内部机制

### 任务派发数据流（订阅模型）

`register_task` 订阅（`rpc/mod.rs:222`）接收 sink，创建 `mpsc(32)`，分配 `reg_id=Uuid::new_v4()`，`add_session`。`TaskManager::send_event`（`rpc/mod.rs:445`）在读锁下克隆 `mpsc::Sender` 并 `try_send` 一个 `TaskEvent`；agent 通过转发的订阅接收。客户端断开时，转发任务退出并按 `reg_id` 门控调用 `remove_session`。

### 阻塞等待者生命周期

`create_task_blocking`（`create_task_blocking.rs:113`）在 `send_event` **之前**按 `task_id` 注册 oneshot 等待者，关闭“agent 比 注册 更快返回”的竞态。`upload_task_result`（`upload_task_result.rs:174`）调用 `notify_blocking_waiter(task_id, response)`，移除条目并在 oneshot 上发送。超时移除等待者（及 DB 行）以防泄漏。

### Forwarder 任务生命周期

`TaskRpcImpl::register_task` 派生一个拥有已接受订阅 sink 与 mpsc rx 的 tokio 任务。它将每个 `TaskEvent` 序列化为 `JsonRawValue` 并通过 `sink.send` 推送。任何发送错误或序列化错误中断循环并移除会话。tracing span 被克隆并在 spawn 前丢弃，避免被 spawn 的任务继承 entry guard。

### TaskManager 的锁纪律

`send_event`（`rpc/mod.rs:453`）刻意在读锁内克隆 `Sender` 并在任何 await 前释放锁，使慢速 agent 发送不会阻塞 `add_session`/`remove_session` 写者。它使用 `try_send`（非 `send().await`）——满 32 容量队列立即返回 `Err(104)` 而非挂起 RPC handler，明确为防止慢 agent 挂起 `create_task`。

### `std::sync::RwLock` for `blocking_waiters`

`BlockingWaiters`（`rpc/mod.rs:370`）包裹 `std::sync::RwLock`，因为临界区无 `.await`；register/remove/notify 均用 `write().unwrap_or_else(|e| e.into_inner())` 从中毒恢复，使 panic 的线程不会楔住等待表。

### 全局 TaskManager 单例

`GLOBAL_TASK_MANAGER`（`rpc/mod.rs:374`）是 `OnceLock<Arc<TaskManager>>`，由 `global()` 惰性初始化；`rpc_module()` 总是使用该全局实例，即便被多次调用，所有 RPC handler 共享同一 peer 表。

### MonitoringUuidProvider 注入

`create_task`/`create_task_blocking` 调用 `monitoring_uuid_provider().map(|p| p.get_or_insert(target_uuid))` 并以 `let _ =` 丢弃结果（`rpc/mod.rs:108-110`）。若 provider 未设置或调用失败，任务仍照常派发——这是对权威 agent 表的尽力登记。

### 查询/删除条件编译

query 与 delete 均构建带相同过滤器的并行 select/delete 查询，并复制每个条件的 match 臂。delete 仅在 `Last`/`Limit` 情形用 select 查询获取 ID；否则直接 `delete_many`。Type 条件按后端分歧：Postgres 用 `JSONB ?` 操作符；SQLite 将 JSON 列转为文本并用带转义模式 `%"<key>":%` 的 `LIKE`。

### query 中的流式序列化

`query.rs:207` 通过 `into_json().stream(db)` 流式拉取结果，预分配 `Vec<u8>`（容量 = effective_limit * 500，saturating），并通过 `serde_json::to_writer` 直接序列化每行。`ng_core::utils::server_json` 辅助函数将 `id` 重命名为 `task_id`，并将 JSON 字符串形式的 `task_event_type`/`task_event_result` 字段就地解析为对象。

### `upload_task_result` 中的 TOCTOU 规避

`upload_task_result.rs:86` 按 `(id, uuid, token)` 取一次行。随后的 UPDATE（`upload_task_result.rs:146`）重新施加 `id+uuid+token AND success IS NULL` 并检查 `rows_affected==0`，无需单独再查询即可拒绝并发重复上传。

### 错误路径 DB 回滚

`create_task.rs:125-131` 与 `create_task_blocking.rs:124` 在 `send_event` 失败时删除已插入的行。`create_task_blocking` 在每个错误/超时路径（`create_task_blocking.rs:123, 148, 156`）额外移除阻塞等待者。

## RPC 方法

命名空间 `task`（自定义 jsonrpsee fork，分隔符 `_`，故方法名为 `task_register_task`、`task_create_task`、`task_create_task_blocking`、`task_upload_task_result`、`task_query`、`task_delete`）。除 `register_task` 订阅外，所有方法返回 `RpcResult<Box<RawValue>>` 并包裹在 `rpc_exec!` 中以统一日志。每个 handler 通过 `token_identity(&token)` 开启 `tracing::info_span!(target: "task", ...)` 并对 body 进行 instrument；订阅的转发任务使用克隆的 span，spawn 前丢弃以避免继承。

| 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|
| `register_task`（订阅） | `token: String, uuid: Uuid`（item=`TaskEvent`，unsubscribe=`unregister_task`） | `Task::Listen`，作用于订阅的 `AgentUuid` | agent 订阅其任务流。校验 token（失败拒 code 101），检查 `Task::Listen`（拒绝 102，错误映射），接受 sink，按 uuid 以新 `reg_id` 注册 `mpsc(32)` 会话，spawn 转发协程将 `TaskEvent` 序列化到 WebSocket |
| `create_task` | `token, target_uuid, task_type` | `Task::Create(task_name)` for `Scope::AgentUuid(target_uuid)` | 校验任务类型（`Execute` cmd 非空），授权，插入带随机 10 字符 `task_token` 的任务行，确保目标 UUID 在 monitoring_uuid 中，经 `send_event` 派发；发送失败则回滚行并返回 `AgentConnectionError`。返回 `{"id":task_id}` |
| `create_task_blocking` | `token, target_uuid, task_type, timeout_ms` | `Task::Create(task_name)` **且** `Task::Read(task_name)` for `Scope::AgentUuid(target_uuid)` | 同 `create_task` 但派发前注册阻塞等待者，超时钳制到 300s，在 oneshot 上等待 agent 上传的 `TaskEventResponse`，返回完整序列化响应。发送失败/超时/通道关闭：移除等待者并删除 DB 行 |
| `upload_task_result` | `token, task_response: TaskEventResponse` | `Task::Write(task_name)` for `Scope::AgentUuid(agent_uuid)`；super-token 旁路 | 两阶段权限（super-token 旁路，否则先做通用 any-`Task::Write` 预检规避时序攻击，再做精确 `Task::Write(task_name)`）。校验三元组；拒绝重复上传（success 已设）；以 `success IS NULL` 守卫更新 timestamp/success/error_message/task_event_result；唤醒阻塞等待者。返回 `{"id":task_id}` |
| `query` | `token, task_data_query: TaskDataQuery` | 每个 Uuid 条件 -> `Task::Read(task_name)` for `Scope::AgentUuid(uuid)`；无 Uuid 条件则 `Scope::Global`；无 Type 条件则全部 11 种类型，否则仅命名类型 | 流式输出匹配行为 JSON 数组。默认上限 1000（asc）；`Last` -> 1 行 desc；`Limit(n)` 钳到 10000 desc。重命名 `id->task_id` 并解析 JSON 字符串字段。返回 `RawValue` JSON 数组 |
| `delete` | `token, conditions: Vec<TaskQueryCondition>` | 同 query 但 `Task::Delete` | 删除匹配行。`Last`/`Limit`：先选 ID（Timestamp/Id desc）再 `delete_by_id.is_in(ids)`；否则带过滤 `delete_many`。Limit 钳到 10000。返回 `{"success":true,"deleted":N,"condition_count":N}` |

错误处理：每个 handler 将逻辑包裹在返回 `anyhow::Result` 的内部 async 块中，再经 `anyhow_to_nodeget_error` 映射为 jsonrpsee `ErrorObject::owned`；query/delete 还通过 `anyhow_error_to_raw` 附带额外 JSON 数据载荷。

## 数据库实体 / 迁移

ng-task **不拥有** `task` 实体（定义于 `crates/ng-db/src/entity/task.rs`），但通过 SeaORM 直接读写。

| 表名 | 列 | 约束 / 索引 / 关系 | 备注 |
|---|---|---|---|
| `task` | `id`（i64，primary_key）、`uuid`（Uuid）、`token`（String）、`cron_source`（Option<String>）、`timestamp`（Option<i64> ms）、`success`（Option<bool>）、`error_message`（Option<String>）、`task_event_type`（Json——序列化的 `TaskEventType`）、`task_event_result`（Option<Json>——序列化的 `TaskEventResult`） | Relation 枚举为空——task **无** FK 关系 | `delete.rs` 明确指出删除任务不影响 monitoring_uuid，也无需 cache reload。`success=NULL` 表示任务仍在运行/挂起 |

无迁移步骤归 ng-task 所有（迁移位于 ng-db）。`task_event_type` 与 `task_event_result` 以 JSON 列存储；Postgres 用 `JSONB ?` 查类型键，SQLite 将列转文本后 `LIKE`。

## Crate 内部约定

- **Feature gate**：`default = []` 仅暴露 types（`TaskEventType`、`TaskEvent`、`TaskEventResult`、`TaskEventResponse`、各参数/结果结构体、`query` 模块）；`server` feature 门控 rpc 模块、`TaskManager`、`MonitoringUuidProvider`、注入 setter。
- **自定义 jsonrpsee fork**：命名空间分隔符为 `_` 非 `.`，方法名 `task_*`，经 `#[rpc(server, namespace = "task")]` + `#[method/subscription(name = ...)]` 定义。
- **统一日志**：所有 RPC 方法返回 `RpcResult<Box<RawValue>>` 并包裹在 `rpc_exec!` 中（`register_task` 订阅除外）。
- **tracing 目标**：`target: "task"`；订阅的转发任务用克隆 span，spawn 前丢弃以避免继承 entry guard。
- **Serde**：枚举 `TaskEventType`、`TaskEventResult`、`DnsRecordType`、`TaskQueryCondition` 均用 `rename_all="snake_case"`。
- **错误映射**：handler 内部用 `anyhow::Result`，再经 `anyhow_to_nodeget_error` 映射；query/delete 额外经 `anyhow_error_to_raw` 附带 JSON 数据载荷。
- **注释**：doc 与行内注释为中文；枚举变体 doc 为中文。
- **task_id 类型**：DB 存 `i64`，但在 `TaskEvent`/`task_id` 字段经 `.cast_unsigned()` 暴露为 `u64`；按 `u64` 输入过滤时用 `.cast_signed()`。
- **常量复刻**：`MAX_LIMIT=10_000` 与 `DEFAULT_LIMIT=1000` 镜像 `crontab_result`/`js_result` 约定。

## 注意事项与陷阱

- **切勿**在新增任务变体时只改一处：`TaskEventType::task_name`（`crates/ng-task/src/types/mod.rs:153`）与 `TaskEventResult::task_name`（`crates/ng-task/src/types/mod.rs:261`）是两个独立 const fn，必须手动同步；此外还要更新 `permission_field`、`validate_task_type` 的 match、以及 `query.rs`/`delete.rs` 中各 11 项的 `all_task_types` 数组。
- **`all_task_types` 数组必须与 `TaskEventType` 同步**：`crates/ng-task/src/rpc/query.rs:40`（及 delete.rs）的硬编码数组当前列 11 种（`ping/tcp_ping/http_ping/http_request/web_shell/execute/read_config/edit_config/ip/version/dns`），**不含 `SelfUpdate`**。新增任务类型时若不同步此数组，无 Type 条件的 query/delete 会静默缺失对该类型的权限覆盖。
- **`TaskResponseItem` 未被使用**：`crates/ng-task/src/types/query.rs:48` 定义但 query RPC 并不序列化进它（手工塑形 JSON）。`task_id` 在此为 `i64` 而 `TaskEvent` 为 `u64`——若将 query.rs 切换为使用此结构体，必须协调类型。
- **`send_event` 用 `try_send`**：`crates/ng-task/src/rpc/mod.rs:467`——满 32 容量队列或未知 UUID 均 `Err(104)`，使 `create_task` 返回 `AgentConnectionError`（104）并回滚刚插入的行。调用方必须重试；这是有意的（防 RPC handler 挂起），但负载下可能导致任务抖动。两个错误仅能从 message 字符串区分。
- **`set_monitoring_uuid_provider` 静默忽略二次调用**：`crates/ng-task/src/rpc/mod.rs:67` 以 `let _ = .set()` 设置，第一个 provider 永久获胜；进程内重启或二次注册会丢弃新 provider 且无错误。
- **monitoring_uuid 登记失败被忽略**：`crates/ng-task/src/rpc/mod.rs:108` 以 `let _ =` 丢弃 `get_or_insert` 错误，任务照常派发——目标 agent 可能不在权威 agent 表中，影响依赖该表的其他子系统。
- **`create_task_blocking` 超时的竞态**：`crates/ng-task/src/rpc/create_task_blocking.rs:156` 超时时移除等待者并删除 DB 行，但已在途（超时前 agent 已收到事件）的迟到结果随后在 `upload_task_result` 的三元组查找中失败（NotFound），agent 的工作被静默丢弃——可接受但需知晓。
- **`create_task_blocking` 回滚错误被吞**：`crates/ng-task/src/rpc/create_task_blocking.rs:124` 的 send_event 错误路径中 DB 行删除错误以 `let _ = ...exec(db).await` 静默丢弃（不同于 `create_task` 会记录日志）；失败的回滚会留下 `success=NULL` 的孤儿挂起行。
- **Type 条件可能误判**：`crates/ng-task/src/rpc/query.rs:151`——JSONB 键存在性检查非值匹配。Postgres `?` 测顶层键；SQLite 的 LIKE 模式 `%"<type_key>":%` 在同子串出现在嵌套值/字符串内时**会假阳性**。镜像 `crontab_result` 行为。
- **`task_id` 无符号转换**：`crates/ng-task/src/rpc/create_task.rs:113` 用 `cast_unsigned()` 构造 `TaskEvent.task_id`（u64），但 DB 列是 `i64`；正常自增无碍，但手工插入的负 id 在线路上会回绕成巨大 u64。

## 依赖关系

ng-task 是业务 crate，依赖 `ng-core`（错误类型、`NodegetError`、`Scope`/`Permission`/`Task`、`utils::server_json`、token 身份）、`ng-db`（`task` 实体、`RpcHelper`、`try_set_json`、DB 连接全局）、`ng-infra`（`rpc_exec!` 宏、`token_identity`）以及 `ng-token`（`PermissionChecker`、`TokenOrAuth`、token 校验、`generate_random_string`）。其 `default = []` 配置使其可被 agent 安全依赖（agent 仅用 types）；`server` feature 由 server binary 启用。server binary 在 `serve.rs` 注册 `TaskMonitoringUuidProvider`（实现 `MonitoringUuidProvider`），并将 `rpc_module()` 合并入 `build_modules()`。agent 端消费 `TaskEvent`/`TaskEventResponse` 类型与 `permission_field` 来本地授权执行。
