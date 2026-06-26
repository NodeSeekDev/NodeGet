# ng-terminal：WebSocket 终端中继

> 概览：ng-terminal 是一个**仅服务器端**的 WebSocket 中继，把 User 浏览器里的终端桥接到远端 Agent 的 shell。它**不是** JSON-RPC；只暴露一个 axum HTTP 路由 `/terminal`，在该路由上升级 WebSocket。Agent 先连接（携带 `task_token`+`task_id`+`terminal_id`），以 `(agent_uuid, terminal_id)` 为 key 注册一个 `SessionSlots` 句柄；User 随后连接（携带 `token`+`terminal_id`），经过 `PermissionChecker`（`Terminal::Connect` scope）授权后取走 Agent slot 的 receiver 半边，拼成一条双向管道。所有实际功能都在 `server` feature 之后；默认 features 下整个 crate 为空。

## 模块结构

```
crates/ng-terminal/src/
├── lib.rs          # Crate 根与唯一公共 API 闸门；默认 features 下为空
├── terminal.rs     # 核心中继引擎：共享状态、查询参数、axum 路由、WS 升级处理器、Agent/User 连接处理器
├── auth.rs         # User 侧权限校验：验证 Token 是否拥有目标 Agent 的 Terminal::Connect 权限
└── check_agent.rs  # Agent 侧身份校验：确认 (task_id, agent_uuid, task_token) 三元组对应一条未完成的 WebShell 任务
```

- `lib.rs:11` 声明了三个 `#[cfg(feature = "server")] mod ...;`（`auth` / `check_agent` / `terminal`），仅在 server feature 启用时编译；默认 features 下 crate 没有任何模块。
- `lib.rs:18` 通过 `pub use` 重导出 `check_terminal_connect_permission`、`check_agent`，以及 `{SessionSlots, TerminalParams, TerminalSessionKey, TerminalState, router, terminal_ws_handler}`——全部位于 `#[cfg(feature = "server")]` 之后，构成 crate 的完整公共表面。

## 公共 API

| 名称 | 签名 | 行为 |
|---|---|---|
| `router` | `pub fn router() -> axum::Router` | 构建 `axum::Router::new().route("/terminal", routing::get(terminal_ws_handler)).with_state(TerminalState { ... })`（`terminal.rs:94`）。进程内唯一的共享 sessions map 来源；服务器二进制必须挂载此 router，**切勿**在别处再建 `TerminalState`，否则 sessions 不会共享。 |
| `terminal_ws_handler` | `pub async fn terminal_ws_handler(ws: WebSocketUpgrade, Query<TerminalParams>, State<TerminalState>) -> impl IntoResponse` | 在 debug 级别记录日志，设置 `max_frame_size(1MB)` 与 `max_message_size(4MB)`（`terminal.rs:119-120`），随后 `on_upgrade -> handle_socket`（`terminal.rs:113`）。超大 frame/message 在到达中继逻辑之前就被 axum 拒绝。 |
| `TerminalState` | `pub struct { pub sessions: Arc<RwLock<HashMap<TerminalSessionKey, SessionSlots>>> }`（derive `Clone`） | `terminal.rs:39`。持有一个进程内所有活跃 Agent 注册的终端 session。在 `router()` 内创建一次（`terminal.rs:97-99`），通过 axum State 传递；Clone 廉价（仅 Arc）。 |
| `TerminalSessionKey` | `pub struct { pub agent_uuid: Uuid, pub terminal_id: Uuid }`（derive `Clone, Debug, Hash, Eq, PartialEq`） | `terminal.rs:46`。HashMap 的 key；复合身份保证每个 Agent 下每个终端会话唯一。 |
| `SessionSlots` | `pub struct { pub tx_to_agent: mpsc::Sender<Message>, pub rx_from_agent: Option<mpsc::Receiver<Message>>, pub task_token: String }` | `terminal.rs:59`。"半开"中继句柄。Agent 连接时插入两半；User 连接时 `take()` 走 `rx_from_agent`（`None` 表示已有 User 附着）。`task_token` 在初次 `check_agent` 之后**不再重新校验**。 |
| `TerminalParams` | `#[derive(Deserialize)] pub struct { pub agent_uuid: String, pub task_id: Option<u64>, pub task_token: Option<String>, pub terminal_id: Option<Uuid>, pub token: Option<String> }` | `terminal.rs:73`。`/terminal` 的 Query 提取器；除 `agent_uuid` 外全部可选，使同一 query shape 同时服务 Agent 与 User 两端。 |
| `check_terminal_connect_permission` | `pub async fn check_terminal_connect_permission(token: &str, agent_uuid: &str) -> anyhow::Result<()>` | `auth.rs:33`。解析输入、获取全局 `PermissionChecker`，先尝试 `Scope::AgentUuid`，失败回退 `Scope::Global`（OR 语义）。任一返回 `true` 即 `Ok(()))`；否则 `Err(NodegetError::PermissionDenied)`。 |
| `check_agent` | `pub async fn check_agent(agent_uuid: String, task_token: String, task_id: u64) -> anyhow::Result<bool>` | `check_agent.rs:32`。查 task 表的 `id+uuid+token+TaskEventResult-is-null`；无行返回 `Ok(false)`，存在且为 `TaskEventType::WebShell(_)` 返回 `Ok(true)`，存在但非 WebShell 返回 `Err(PermissionDenied)`。 |

## 关键类型与常量

- `terminal.rs:34` — `TERMINAL_CHANNEL_BUFFER_SIZE: usize = 4096`：User→Agent 与 Agent→User 两条 mpsc channel 的同一容量（创建于 `terminal.rs:224-226`）。有界，防止单边慢消费导致无界内存增长。
- `terminal.rs:39` — `TerminalState { sessions: Arc<RwLock<HashMap<TerminalSessionKey, SessionSlots>>> }`，derive `Clone`。
- `terminal.rs:46` — `TerminalSessionKey { agent_uuid: Uuid, terminal_id: Uuid }`，derive `Clone, Debug, Hash, Eq, PartialEq`。
- `terminal.rs:59` — `SessionSlots { tx_to_agent, rx_from_agent: Option<_>, task_token: String }`。`rx_from_agent` 的 `Option` 即**单租户 User 锁**。
- `terminal.rs:73` — `TerminalParams`：`#[derive(Deserialize)]`，`agent_uuid: String` 必填，其余 `Option`。
- `terminal.rs:373` — `SlotResult`（`handle_user` 内联私有 enum）：`Got(mpsc::Sender<Message>, mpsc::Receiver<Message>) | AlreadyAttached | NotFound`，表示尝试取 Agent 侧 slot 的三态结果。
- 路由常量：`max_frame_size = 1MB`、`max_message_size = 4MB`（`terminal.rs:119-120`，硬编码，**不**来自 config）。
- 错误码：`108 = Invalid Input`、`102 = Permission Denied`（由 `generate_error_message(code, &msg)` 生成 JSON 错误体）。

## 内部机制

### Agent-first / User-second 会合（rendezvous）

Agent 先连接（`handle_agent`）：`check_agent` 校验 `task` 表，通过后 Agent 创建 `SessionSlots`（两条 mpsc 的两半）并以 `(agent_uuid, terminal_id)` 注册。User 后连接（`handle_user`）：权限校验通过后 `take()` 走 slot 里的 `rx_from_agent`，并 clone 一份 `tx_to_agent`。此后跨两条 socket 的四个 tokio task 组成中继：

- User WS recv → `tx_to_agent` → Agent 的 `rx_from_user` → Agent WS send
- Agent WS recv → `tx_to_user` → User 的 `rx_from_agent` → User WS send

有界 channel（4096）提供背压。slot 的 `rx_from_agent` 为 `Option` 即单租户 User 锁。

### 对称 select! + abort 清理

`handle_agent` 与 `handle_user` 都 spawn 两个 JoinHandle 并用 `tokio::select!{biased; recv_task; send_task}` 等待——先结束的一端胜出，随后**两个** task 都被显式 `.abort()`（`terminal.rs:295-296` 与 `467-468`）。原实现只 await `send_task` 且只 drop handle 不 abort，会泄漏：User 断开时 Agent 的 `send_task` 仍 parked 在 `ws_receiver.next()`，`handle_agent` 永不返回，session 留在 map 里，相同 `terminal_id` 重连会被当作 "already attached" 拒绝。`select!` + `abort` + `remove` 序列（外加 `handle_user` 里的 session_key 提升修复）是该泄漏的官方修复。

### Entry API 原子插入

`handle_agent`（`terminal.rs:231-248`）在**同步块**内使用 `sessions.entry(session_key)`：`Occupied` → 返回 false（调用方以 108 "already active" 拒绝）；`Vacant` → 插入 `SessionSlots`。块定界确保 `std::sync::RwLockWriteGuard` 在任何 `.await` 之前 drop（guard 不是 `Send`）。锁中毒通过 `unwrap_or_else(PoisonError::into_inner)` 恢复。Entry API 避免 `contains_key`+`insert` 的 TOCTOU。

### handle_user 中 session_key 的生命周期 / 提升修复

`handle_user`（`terminal.rs:389-403`）把 `parsed_uuid` 与 `session_key` 的构造**提升到** slot_result 块之外（注释见 `378-380`），因为 `session_key` 在 `select!` 之后还要用来 remove slot。在写锁内的 `get_mut` 中 `take()` 走 `rx_from_agent`。早先的 bug 把 `session_key` 声明在内层作用域，导致它被 drop、断开后的 remove 无法引用——slot 永久残留。

### 幂等 remove + 级联拆除

两个 handler 在断开后都于写锁下 `sessions.remove(&session_key)`（`terminal.rs:299-305` 与 `469-475`），视 remove 为幂等。关键点：User 断开会移除 slot，从而 drop 掉 User clone 的 `tx_to_agent` Sender；当 Agent 的 `rx_from_user` 的所有 Sender 都消失时，Agent 的 `recv_task` 观察到 `None` 退出，结束 Agent 的 `select!`，abort 其 task，触发它自己（幂等）的 remove。**User 断开通过 channel 关闭级联到 Agent 拆除，而非任何显式信号。**

### check_agent 四条件任务查找

`check_agent.rs:32` 用 `id+uuid+token+TaskEventResult-is-null` 查 `task::Entity`。无匹配行返回 `Ok(false)`（被 `handle_agent` 当作 "Permission Denied: Invalid Task Token or ID"，码 102）；仅当存在行**且**其 `task_event_type` 反序列化为 `TaskEventType::WebShell(_)` 才返回 `Ok(true)`。非 WebShell 任务类型 → `PermissionDenied`。完成以 `TaskEventResult` 非空为信号（CLAUDE.md "soft delete / not-yet-completed" 语义）。

### 双 scope OR 权限解析

`auth.rs:33` 先尝试 `Scope::AgentUuid(agent_uuid)` + `Permission::Terminal(Terminal::Connect)`（经由注入的 `PermissionChecker`，即 `require_permission_checker` OnceLock）。失败回退到 `Scope::Global` + `Terminal::Connect`。任一返回 `true` 短路到 `Ok(())`。这与工作区 RBAC 一致：`TokenOrAuth`（key:secret 或 username|password）携带 `Vec<Limit>` 的 scope+permission；super-token（id=1，常量时间）在 checker 层绕过，不在此处。

### 硬编码 WebSocket 大小限制

`terminal_ws_handler` 设 `max_frame_size(1MB)` 与 `max_message_size(4MB)`（`terminal.rs:119-120`），匹配 CLAUDE.md "terminal WebSocket: max frame 1MB, max message 4MB"。这些常量**不**来自 config。

## Crate 内部约定

- **Feature gate 模式（与其它 crate 不同）**：`ng-core` 是**唯一**无条件依赖（始终可用）；其它所有 crate 与外部依赖（`axum`、`tokio`、`serde`、`uuid`、`tracing`、`sea-orm`、`ng-db`、`ng-infra`、`ng-task`）都是可选的，全部位于 `server` feature 之后。默认 features 下 crate **什么都不导出**（`lib.rs` 整体被 `#[cfg(feature = "server")]` 守卫）。
- server feature 下仅启用 `ng-core/for-server`（**非** for-agent）——terminal 是服务器专用；agent crate 完全不依赖 ng-terminal。
- `sea-orm` 配置 `default-features=false` 加 `runtime-tokio-rustls`、`sqlx-sqlite`、`sqlx-postgres`、`macros`（匹配服务器双 DB 支持）。
- `uuid` 配置 `std+serde+v4+v5+fast-rng`（v5 用于别处 NodeGet UUID 生成；terminal 仅用 `parse_str`）。
- 所有 tracing 调用统一使用 `target: "terminal"`（三个模块一致；`lib.rs` 无日志）。
- 日志级别约定：`trace` = 进入 `auth`/`check_agent` 的每次调用；`debug` = 每连接路由 + AgentUuid/Global scope 解析 + "not found" 数据结果；`info` = session 生命周期（connecting/disconnected）；`warn` = 拒绝与权限否定；`error` = 在关闭中的 socket 上 send 失败。
- 中文行内注释贯穿全 crate（匹配 CLAUDE.md 工作区约定）；doc 注释中英文混用（职责/协作关系用中文，函数级 `///` 用英文）。
- 错误信号约定：拒绝连接时，handler **总是**先 `Message::Text` 发送一个由 `generate_error_message(code, &msg)` 生成的 JSON 错误体再返回（码：`108=Invalid Input`、`102=Permission Denied`）。**不**显式发起 close-handshake——socket 直接被 drop。
- UUID 解析约定：用 `Uuid::parse_str`；`handle_agent`/`handle_user` 中失败走 `reject_with_error` 或 warn+return；`auth`/`check_agent` 中返回 `NodegetError::ParseError`。
- RwLock 中毒一律用 `.unwrap_or_else(std::sync::PoisonError::into_inner)` 恢复——crate 永不因中毒锁 panic。
- 无 `#[rpc]` 宏——ng-terminal 暴露的是 HTTP/WebSocket axum router，而非 JSON-RPC；由服务器二进制把 `ng_terminal::router()` 接入 axum Router（见 CLAUDE.md HTTP 路由表）。
- Serde：`TerminalParams` derive `Deserialize` 供 axum Query 提取；任何本地类型都**不** derive `Serialize`（错误体通过 `generate_error_message` 助手序列化）。

## 注意事项与陷阱

- **维护者切勿**重复构造 `TerminalState`（`crates/ng-terminal/src/terminal.rs:36`）。它只在 `router()` 内创建一次，sessions map 靠同一个 `TerminalState`（`Arc<RwLock<...>>`）经 axum State 共享。若未来改动造了第二个 `TerminalState`（例如把 router 合并进另一个 `.with_state`），sessions map 会静默分片，Agent/User 会合会以 "Terminal session not found" 失败。
- `TERMINAL_CHANNEL_BUFFER_SIZE` 硬编码 4096 无 config（`crates/ng-terminal/src/terminal.rs:34`）。快生产者配慢消费者会施加 Tokio 有界 channel 背压（`send().await` 阻塞）而非丢消息——可能拖停某条中继方向；这是有意为之，但调优终端吞吐时需知此。
- `SessionSlots.task_token` **仅在** Agent 连接时校验一次（`handle_agent` 调 `check_agent`，`terminal.rs:182`），后续操作**不**再校验，也**不**用于授权 User 侧。**切勿**依赖它做逐消息认证（`crates/ng-terminal/src/terminal.rs:243`）。
- 清理正确性依赖 `select!{biased}` + 显式 `.abort()` **两个** task（`crates/ng-terminal/src/terminal.rs:289`）。移除 abort（或回退到只 await 一个 task）会重新引入文档化的 session/Agent-WS 泄漏：User 断开时 `handle_agent` 不返回，slot 残留，相同 `terminal_id` 重连被当作 "already attached" 拒绝。
- `handle_user` 中 `parsed_uuid`/`session_key` **必须**留在 slot_result 块之外的作用域，以便 `select!` 后的 remove 能引用（`crates/ng-terminal/src/terminal.rs:385`）。把构造移回内层作用域会重新引入 issue #152 的 slot 泄漏 bug（原代码断开时无法 remove slot）。
- `std::sync::RwLockWriteGuard` 不是 `Send`，**不得**跨 `.await` 持有（`crates/ng-terminal/src/terminal.rs:263`）。所有 map 变更都在 `{ ... }` 同步块内（如 `231-248`、`299-305`、`389-403`、`469-475`）。在持写锁时加 `.await` 会编译失败（或 parking_lot 下死锁）；保持 map 访问在紧凑的同步作用域内。
- 锁中毒恢复（`crates/ng-terminal/src/handle_agent:236`）：每次写都用 `.unwrap_or_else(std::sync::PoisonError::into_inner)`，意味着某连接里的 panic 中毒了锁会被静默吞掉——sessions 可能处于不一致状态但服务器继续运行。调试 "丢失" sessions 时应考虑先前是否有 panic 中毒了锁。
- `task_id` 是 `u64` 但 `task::Column::Id` 是有符号列，故用 `.cast_signed()`（`crates/ng-terminal/src/check_agent.rs:46`）。若调用方传入大于 `i64::MAX` 的 task_id，会在 SQL 层 wrap/溢出；实践中 task ID 很小，但若 ID 类型变宽，此 cast 是个 foot-gun。
- `check_agent` 用 `TaskEventResult.is_null()` 作为 "未完成" 谓词（`crates/ng-terminal/src/check_agent.rs:65`）。已产生结果的任务（终端附着过一次后再试）会不满足该过滤、返回 `Ok(false)` → "Permission Denied: Invalid Task Token or ID"。**复用已完成的 WebShell task id 开新终端 session 会被拒绝**——服务器必须为每个终端 session 生成新 task。
- `auth.rs` 与 `check_agent.rs` 依赖 `require_permission_checker()` 和 `ng_db::get_db()`——都是 OnceLock 全局，仅由服务器二进制在启动时初始化（`crates/ng-terminal/src/auth.rs:44`）。在未设置处调用 `check_terminal_connect_permission` 或 `check_agent` 会返回 `Err`（如 `DatabaseError 'DB not initialized'` 或 checker 未设置错误）。这些函数只有经服务器挂载的 router 触达才有意义。
- Agent 与 User **仅靠** query 中同时存在 `task_token` 与 `task_id` 来区分（`handle_socket` `terminal.rs:142`）。User 若误在 query 里带了 `task_token`+`task_id` 会被当 Agent 路由，命中 `check_agent`（会被拒绝，但永不到达 User 权限路径）。**保持** User 客户端的 query 不带这两个参数。
- User 侧无效 `agent_uuid` 的静默关闭（`crates/ng-terminal/src/auth.rs:33`）：`handle_user` 在调用 `check_terminal_connect_permission` 之前自己解析 UUID（`terminal.rs:381-384`），失败时**直接 return**，只 warn 不发 JSON 错误体。所以 User 侧的非法 `agent_uuid` 是静默关闭（无错误消息到达客户端），与 `handle_agent` 发显式 108 错误不同。

## 依赖关系

ng-terminal 是服务器专用 crate，在 `server` feature 下依赖 `ng-core`（无条件，仅启用 `for-server`）、`ng-db`、`ng-infra`、`ng-task` 以及 `axum`/`tokio`/`serde`/`uuid`/`tracing`/`sea-orm` 等外部 crate。它不参与 JSON-RPC 命名空间组合，而是由服务器二进制在构建 axum Router 时把 `ng_terminal::router()` 挂到 `/terminal` 路径（见 CLAUDE.md HTTP 路由表）。agent crate **不**依赖 ng-terminal。
