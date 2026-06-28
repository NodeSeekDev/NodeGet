# nodeget-server — 服务端二进制（进程入口与组合根）

> 概览：`nodeget-server` 是 NodeGet 的服务端二进制，是整个系统的**组合根**（composition root）。它负责进程入口、CLI 子命令分发、`tracing` 日志体系搭建、仅服务端持有的 `nodeget-server` JSON-RPC 命名空间、RPC 计时中间件、axum 路由组装（RPC + 静态文件 + WebDAV + JS Worker HTTP 路由 + Terminal WebSocket），以及所有基于 `OnceLock` 的 trait 提供者注入。它将每个 `ng-*` crate 的 RPC 模块合并为一个 `RpcModule`，并把具体实现（`ServerPermissionChecker`、`TaskMonitoringUuidProvider`、`JsWorkerServiceImpl`、`CronJsWorkerScheduler`）注入到业务 crate 的抽象接缝中。本 crate 没有 `lib.rs`，是纯二进制 crate。

## 模块结构

```
server/
├── build.rs                          # 32-bit ARM 目标链接 libatomic（QuickJS 64-bit 原子操作）
├── src/
│   ├── main.rs                       # 进程入口；Tokio runtime；CLI 分发；热重载外层循环
│   ├── logging/mod.rs                # 四层 tracing 日志（console / JSON 文件 / 内存环 / 实时流）
│   ├── rpc_nodeget.rs                # nodeget-server RPC 命名空间实现；合并所有 ng-* RPC 模块
│   ├── rpc_timing.rs                 # RPC 计时中间件（call / batch / notification）
│   └── subcommands/
│       ├── mod.rs                    # 子命令模块声明 + init_or_skip_super_token 公共助手
│       ├── serve.rs                  # serve 子命令；缓存初始化；trait 注入；axum 路由；shutdown
│       ├── init.rs                   # init 子命令：生成 Super Token 后退出
│       ├── get_uuid.rs               # get_uuid 子命令：打印 server_uuid
│       └── roll_super_token.rs       # roll_super_token 子命令：交互式轮换 Super Token
```

## 入口与启动流程

### `main()` — `server/src/main.rs:34`

- 在 `dhat-heap` feature 启用时创建 `dhat::Profiler`（`ALLOC` 全局分配器在 `server/src/main.rs:26`）。
- 打印 `Starting nodeget-server`。
- 构建多线程 Tokio runtime，使用 `global_queue_interval(3)` 与 `enable_all()`；构建失败直接 `panic`。
- `block_on(async_main)`。

### `async_main()` — `server/src/main.rs:59`

按以下顺序执行：

1. `ng_js_runtime::init_server_runtime(current Handle)` —— 先于一切业务初始化 JS runtime，确保 JS 全局对象在其它 crate 使用前就绪。
2. 通过 `ServerArgs::par()` 解析 CLI。
3. **Version 子命令短路**：在解析配置、初始化日志之前就打印 `NodeGetVersion::get()` 并 `return`（`server/src/main.rs:65`）。
4. 设置 server config path 与 `init_reload_notify`。
5. `ServerConfig::get_and_parse_config` —— 失败时 `eprintln` + `exit(1)`。
6. `logging::init(config.logging)`。
7. `set_server_config` 写入全局配置；失败 `exit(1)`。
8. 按 `ServerArgs` 分发：

| 子命令       | 处理                                                                                                                                                                       |
|--------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Serve`      | `init_db_connection` + 外层热重载循环调用 `serve::run`（见下文「热重载生命周期」）                                                                                          |
| `Init`       | `init_db_connection` 后进入 init 子命令                                                                                                                                    |
| `RollSuperToken` | `init_db_connection` 后进入 roll_super_token 子命令                                                                                                                    |
| `GetUuid`    | 不初始化 DB，直接打印                                                                                                                                                       |
| `Version`    | 打印版本（重复 arm，实际由前面短路处理，不可达）                                                                                                                             |

### 热重载外层循环 — `server/src/main.rs:101`

```
loop {
    serve::run(&config).await;          // 仅在 shutdown 或 reload 信号时返回
    重新解析 config;
    set_server_config;
    ng_static::reload_static_path();
    ng_static::clear_dav_handler_cache();
    config = reloaded;
}
```

- 配置解析失败时记录错误并 `continue`（**保留旧 config**）。
- `set_server_config` 失败时记录错误并 `continue`。
- 成功时记录 `Config hot reload applied`。
- 此循环**无退出条件、无重试上限、无 backoff**（见「注意事项」）。

### `init_db_connection()` — `server/src/main.rs:157`

- 在读锁下从全局配置读取 `[database]` 段。
- 构建 `DbConnectionConfig`，对 `None` 字段填入默认值：`connect_timeout` / `acquire_timeout` / `idle_timeout` = **3000ms**，`max_lifetime` = **30000ms**，`max_connections` = **10**。
- 调用 `ng_db::init_db_connection`，失败 `panic`。

### 子命令

| 子命令            | 函数 / 行号                              | 行为                                                                                                                                       |
|-------------------|------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `init`            | `run` `server/src/subcommands/init.rs:11` | 调用 `init_or_skip_super_token().await`，记录 `Initialization completed, exiting`（target `server`），随后进程随 `async_main` 返回退出。     |
| `get_uuid`        | `run` `server/src/subcommands/get_uuid.rs:8` | 同步打印 `config.server_uuid`；不初始化 DB。                                                                                                |
| `roll_super_token`| `run` `server/src/subcommands/roll_super_token.rs:17` | `prompt_yes_or_no`（`server/src/subcommands/roll_super_token.rs:45`，接受 `y/yes`/`n/no`，无效输入重试）；确认后调用 `ng_token::super_token::roll_super_token`，Ok 时 stdout 打印凭据，Err 时 `panic`。 |
| `serve`           | `run` `server/src/subcommands/serve.rs:51` | 见下文「serve::run」。                                                                                                                       |

### `init_or_skip_super_token()` — `server/src/subcommands/mod.rs:22`

- 调用 `ng_token::super_token::generate_super_token()`，Err 直接 `panic`。
- `Some(token)` 时向 **stdout** 打印 `Super Token: {}` 与 `Root Password: {}`。
- `None` 时以 target `server` 记录 `Super Token already exists, skipped`。
- **凭据绝不走 tracing**（见「注意事项」）。

### `serve::run()` — `server/src/subcommands/serve.rs:51`

1. 安装 rustls `aws_lc_rs` default_provider。
2. `init_or_skip_super_token`。
3. 初始化全部缓存：`TokenCache`、`MonitoringUuidCache`、`StaticHashCache`、`MonitoringLastCache`、`StaticCache`、`CrontabCache`、`runtime_pool`、`monitoring_buffer`、`DbRegistryManager`（`db_path` 默认 `./db/`）。
4. 注入四个 trait 提供者：`ServerPermissionChecker`、`TaskMonitoringUuidProvider`、`JsWorkerServiceImpl`、`CronJsWorkerScheduler`。
5. 构建合并的 RPC 模块 + `RpcTimingMiddleware(TRACE)` + `ServerConfig`（默认 `max_conns` **100**、`resp` **100MiB**、`req` **10MiB**）。
6. 构建 axum router。
7. 可选 Unix socket（默认 path `/var/lib/nodeget.sock`）。
8. `tokio::select!`（biased）等待 `serve_future` 完成或 `reload_notify` 信号。

#### 路由组装 — `server/src/subcommands/serve.rs:144`

| 路径                                       | 处理                                                                                                                                                                                                              |
|--------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `/`、`/nodeget/rpc`（任意 method）         | WS upgrade → `rpc_service.call`；否则 GET → 若 `StaticCache.get_http_root` 启用则 `serve_static_file`，否则 `landing_html`，否则 rpc call                                                                            |
| （merge）                                  | `ng_static::router::router()`（静态文件 + WebDAV）                                                                                                                                                                 |
| `/worker-route/...`、`/nodeget/worker-route/...` | 每个前缀 3 个变体（bare、trailing slash、`{*path}`）→ `handle_js_worker_route`                                                                                                                                       |
| （merge）                                  | `ng_terminal::router()`                                                                                                                                                                                           |
| `.fallback()`                              | WS upgrade → rpc；否则 `StaticCache.get_http_root` → `serve_static_file`，否则 rpc                                                                                                                                  |

所有 `rpc_service.call` 错误返回 500 + 记录日志（**不** `unwrap` panic）。

#### select 循环与 shutdown — `server/src/subcommands/serve.rs:407`

- `tls_enabled = tls_cert.is_some() && tls_key.is_some()`。TLS 分支使用 `axum_server::bind_rustls` + `build_http1_only_tls_config`（加载失败 `panic`）。明文分支使用 `TcpListener::bind`（失败 `panic`）+ `axum::serve` 配合 `into_make_service_with_connect_info::<SocketAddr>`。
- `tokio::select!` biased：
  - `serve_future` 完成 → `flush_and_shutdown` + `DbRegistryManager.shutdown` + 5s 超时的 `stop_handle.shutdown` + abort unix task + 清理 socket 文件。
  - `reload_notify` 触发 → 同样的 shutdown，但 `stop_handle.shutdown` 改为**非阻塞 spawn**，使 `run()` 立即返回到 `main` 的热重载循环。
- 两个分支对 `serve_future` 的结果都 `unwrap()`（见「注意事项」）。

#### Unix socket — `server/src/subcommands/serve.rs:379`

- 仅当 `enable_unix_socket`（默认 `false`）开启；默认路径 `/var/lib/nodeget.sock`。
- `bind_unix_listener`（`server/src/subcommands/serve.rs:1062`，创建父目录、移除已存在的 socket 文件、绑定 `UnixListener`）。
- spawn axum serve；绑定失败仅记录日志，**不**阻止 TCP 服务。
- shutdown 时通过 `cleanup_unix_socket_file`（`server/src/subcommands/serve.rs:1084`）移除 socket 文件。

#### TLS / ALPN — `build_http1_only_tls_config` `server/src/subcommands/serve.rs:517`

- 读取 cert/key PEM，用 rustls `pki_types` 解析，构建无客户端认证的 `rustls::ServerConfig` + `single_cert`，再包装为 `axum_server::tls_rustls::RustlsConfig` 返回。
- **强制** `alpn_protocols = [b"http/1.1"]`，避免 HTTP/2 协商开销（文档注释中提到 samply 下 HTTP/2 协商消耗 14–33% 的 tokio worker time）。

## 关键类型与常量

### main.rs

| 项                                            | 行号 | 说明                                                                                                                                                                              |
|-----------------------------------------------|------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `lint cfg`                                    | 7    | `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`；crate 级 allow：`cast_sign_loss`/`cast_precision_loss`/`cast_possible_truncation`/`similar_names`/`dead_code`。         |
| `ALLOC`                                       | 26   | `#[cfg(feature="dhat-heap")] #[global_allocator]`，切换到 `dhat::Alloc` 用于堆 profile。                                                                                          |
| `main`                                        | 34   | 见上。                                                                                                                                                                              |
| `async_main`                                  | 59   | 见上。                                                                                                                                                                              |
| `(hot-reload loop)`                           | 101  | 见上。                                                                                                                                                                              |
| `init_db_connection`                          | 157  | 见上；从 poisoned lock 通过 `.expect` 恢复。                                                                                                                                       |

### logging/mod.rs

| 项                                  | 行号 | 说明                                                                                                                                                                                                    |
|-------------------------------------|------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `DEFAULT_MEMORY_LOG_CAPACITY`       | 38   | 配置 `memory_log_capacity` 为 `None` 时的默认内存日志环容量。                                                                                                                                            |
| `MEMORY_LOG_BUFFER` / `MEMORY_LOG_CAPACITY` | 41   | `OnceLock`，分别持有 `VecDeque<Value>` 环与 `usize` 容量（**容量 0 表示禁用**）。                                                                                                                         |
| `get_memory_logs`                   | 50   | 返回所有缓冲日志条目的克隆 `Vec`；从 poisoned `Mutex` 通过 `into_inner` 恢复；未初始化时返回空 `Vec`。                                                                                                   |
| `init`                              | 76   | 组装 tracing subscriber：`RUST_LOG` 覆盖 `config.log_filter`（默认 `info`）；console 层（`NodeGetFormat`、ANSI）；可选 JSON 文件层（filter 回退到 `console_raw` 再到 `log_filter`）；memory 层（容量 0 禁用）；stream 层。`.init()` 恰好一次。 |
| `MemoryLogLayer`                    | 175  | `on_event` 通过 `JsonFieldVisitor` 收集字段、构建 span 上下文、调用 `build_log_entry`，然后压入有界 `VecDeque`（超容量时 `pop_front`）；Mutex poison 经 `into_inner` 恢复。                                            |
| `JsonFieldVisitor`                  | 256  | tracing `Visit` 实现，把字段收集进 `serde_json::Map`；将 `message` 字段名特殊处理为顶层 `message: Option<String>`。                                                                                            |
| `expand_virtual_targets`            | 317  | 在 EnvFilter 字符串里展开虚拟 `db` 目标：`db=L`/`db` → 保留字面量 + `sea_orm=L` + `sea_orm_migration=L` + `sqlx=L`；非别名透传。                                                                              |
| `remap_target`                      | 350  | 把以 `sea_orm` / `sqlx` 开头的 target 映射回 `db`；其他不变。作用于 console、JSON、memory、stream 四种 formatter，保证 target 命名一致。                                                                          |
| `NodeGetFormat`                     | 365  | console `FormatEvent`：时间戳（`ChronoLocal %Y-%m-%d %H:%M:%S%.3f%:z`）、右对齐 5 字符着色 level（`level_ansi`）、灰色 remap target、`format_fields`、span 链格式 `[name{fields} < name{fields}]`。                |
| `JsonRemapFormat`                   | 457  | JSON 文件 `FormatEvent`，通过 `build_log_entry` 产出单行 JSON + `\n`。                                                                                                                                    |
| `level_ansi`                        | 517  | const fn，返回 (open, reset) ANSI 对：ERROR 红(31)、WARN 黄(33)、INFO 绿(32)、DEBUG 蓝(34)、TRACE 紫(35)。                                                                                                  |
| `build_log_entry`                   | 531  | 共享 builder，产出 JSON `Value`：timestamp（chrono Local ISO）、level、remap target、message、fields object、spans array。使用直接 `Map` 构造，避免 `serde_json::json!` 宏的中间分配。                          |
| `strip_ansi`                        | 565  | 手写 ANSI 转义剥离器；消费 `ESC [` 后到字母终结符之前的参数字节。原因：`FormattedFields<DefaultFields>` 自带 ANSI 码，不能泄漏进 JSON/memory/stream 输出。                                                          |
| `get_stream_log_manager`            | 593  | `OnceLock::get_or_init` 返回单例 `Arc<StreamLogManager>`，被 `init()` 与 `rpc_nodeget::stream_log` 调用。                                                                                                  |
| `StreamLogManager`                  | 605  | 用 `ArcSwap<HashMap<Uuid, StreamLogSubscriber>>` 做无锁读 + `AtomicUsize subscriber_count` 快路径；add/remove 用 `rcu()` 整体交换 map。                                                                       |
| `StreamLogManager::{add_subscriber, remove_subscriber, has_subscribers}` | 627 | `add_subscriber(id, Sender<String>, filter_str)`：展开虚拟目标、解析 `StreamFilter`、rcu 插入、把 `load().len()` 存入 `subscriber_count`；`remove_subscriber` rcu 移除并更新计数；`has_subscribers` 用 `AtomicUsize` Acquire load > 0。 |
| `StreamLogSubscriber`               | 667  | 持有 `tokio::mpsc::Sender<String>`（预序列化 JSON）与 `StreamFilter`；可 Clone。                                                                                                                          |
| `StreamFilter`                      | 683  | 轻量级 EnvFilter 兼容的 target+level 匹配器。`parse()` 按 `,` 分割，支持 `target=level`（最长前缀列表）与裸 level（设 `default_level`，默认 `OFF`）。targets 按 target 长度降序以做最长前缀匹配。`is_enabled(meta)` 查最长匹配前缀，否则用 `default_level`。 |
| `parse_level_filter`                | 744  | 大小写不敏感 `off/error/warn/info/debug/trace` → `LevelFilter`；未知返回 `None`（静默忽略）。                                                                                                               |
| `StreamLogFilter`                   | 768  | Per-layer `Filter<S>`，`enabled()` 返回 `manager.has_subscribers()`。**必须**作为 per-layer filter，不可作为全局 filter。                                                                                  |
| `StreamLogLayer`                    | 795  | `on_event`：无订阅者快速返回；加载 `Arc<HashMap>` 快照；通过 `filter.is_enabled` 收集感兴趣的 Sender；用 `serde_json::json!` 序列化**一次**（注意：此路径仍用 `json!` 宏，与 `MemoryLogLayer` 不同）；`tx.try_send` 非阻塞广播（满则丢弃）。           |

### rpc_nodeget.rs

| 项                                                | 行号 | 说明                                                                                                                                                              |
|---------------------------------------------------|------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Rpc` trait                                       | 34   | `#[rpc(server, namespace = "nodeget-server")]`（`_` 分隔符）。此 trait 定义 server crate 自身提供的方法：`hello`、`version`、`uuid`、`read_config`(super，经 `ng_config` 下游校验)、`edit_config`(super，经 `ng_config` 下游校验)、`database_storage`(super，经 `ng_db` 下游校验)、`log`(super)、`exec_sql`(`NodeGet::ExecSql`)、`get_database_type`(`NodeGet::ExecSql`)、`self_update`(super)；subscription：`stream_log`、`unsubscribe_stream_log`。合并后的 `nodeget-server` 命名空间还会额外包含 ng-monitoring 贡献的 `list_all_agent_uuid`。 |
| `NodegetServerRpcImpl`                            | 109  | 空 marker struct（`Clone`），blanket 实现 `RpcHelper` 与 `RpcServer`。每个 handler 用 `info_span!(target:"server", "nodeget-server::<method>", token_key, username)` 包装。`token_identity(&token)` 在 token 被 move 之前抽出 `(token_key, username)` 用于 span 字段。 |
| `RpcServer::{hello, version, uuid}`               | 117  | `hello` 返回 `'NodeGet Server Is Running!'`；`version` 返回 `serde_json::to_value(NodeGetVersion::get()).unwrap()`；`uuid` 从全局配置读 `server_uuid`，配置缺失或锁中毒返回空 `String`。                  |
| `RpcServer::{read_config, edit_config, database_storage, exec_sql, get_database_type}` | 155  | `read_config`/`edit_config` 委托 `ng_config::server_rpc`（Ok/Err 在 debug/error 记录）；`database_storage`/`exec_sql`/`get_database_type` 经 `rpc_exec!` 宏委托 `ng_db::rpc::nodeget` 子模块。   |
| `RpcServer::log`                                  | 209  | 解析 `TokenOrAuth::from_full_token` 后 `check_super_token`；非 super 返回 `PermissionDenied('Super token required')`。序列化 `logging::get_memory_logs()` 为 `RawValue`；错误经 `ng_db::rpc::to_rpc_error` 映射。 |
| `RpcServer::stream_log`                           | 256  | Super-token 守卫；解析失败以 ErrorObject code **101/102** `reject()` 后返回 `Ok(())`；成功后 accept sink，创建 `mpsc` channel(**512**)，分配随机 `Uuid sub_id`，向 `StreamLogManager.add_subscriber` 注册；drop span guard 后 spawn 转发任务（`rx.recv()` → `RawValue::from_string` → `SubscriptionMessage` → `sink.send`，send 出错 break），退出时 `remove_subscriber`。**不变量**：绝不在持有 manager 写锁时调用 tracing（ArcSwap 下已安全，但纪律保留）。 |
| `RpcServer::self_update`                          | 356  | 见下「self_update 流程」。                                                                                                                                          |
| `get_modules`                                     | 517  | `OnceLock<RpcModule<()>>` 缓存合并模块；首次调用 `build_modules`，后续 clone。                                                                                          |
| `build_modules`                                   | 524  | 创建空 `RpcModule::new(())`，按序 merge（失败 `expect`）：`NodegetServerRpcImpl.into_rpc()` → `ng_monitoring`（其内部已合并 `agent`、`agent-uuid` 与 `nodeget-server::list_all_agent_uuid`）→ `ng_task` → `ng_token` → `ng_kv` → `ng_static::rpc` → `ng_db::rpc::db` → `ng_js_worker` → `ng_crontab`。 |

### rpc_timing.rs

| 项                            | 行号 | 说明                                                                                                                                                                                            |
|-------------------------------|------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `RpcTimingMiddleware<S>`      | 20   | 包装内部 `RpcServiceT`（`service` 字段）+ 一个 tracing `Level`；关联类型委托给 `S`。                                                                                                               |
| `RpcServiceT::call`           | 36   | 抽取 `method_name` 与 request id（owned，在 request 被 move 之前），记录 `Instant`，调用 inner，以配置 level + target `rpc` 记录 `rpc_kind='call'`、method、`elapsed_us`、`id=?request_id`。返回 inner 响应不变。          |
| `RpcServiceT::batch`          | 75   | 收集每条 batch 的 method（解析错误为 `'<invalid>'`），用 `,` 连接（或 `'<empty>'`）；记录 `rpc_kind='batch'`、methods、`elapsed_us`、size；level-gated 懒拼接。                                                          |
| `RpcServiceT::notification`   | 120  | 记录 `rpc_kind='notification'`、method、`elapsed_us`；无 id（notification 无响应）。                                                                                                                  |

### subcommands/serve.rs（其余）

| 项                                            | 行号  | 说明                                                                                                                                                                                                                               |
|-----------------------------------------------|-------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `render_root_html`                            | 547   | 硬编码 HTML landing page，插值 `serv_uuid` 与 `serv_version`（`env! CARGO_PKG_VERSION`）；链接 `dash.nodeget.com`、`nodeget.com`、`github.com/nodeseekdev/nodeget`。                                                                                  |
| `is_websocket_upgrade`                        | 578   | 检查 `Upgrade: websocket`（大小写不敏感）**且** `Connection` 含 `upgrade` 段（大小写不敏感）；用于 `/`、`/nodeget/rpc`、fallback 分支。                                                                                                          |
| `JsRouteHeader / JsRouteInput / JsRouteOutput / JsRouteOutputHeader` | 597   | JS Worker HTTP 路由桥的 Serde 结构；`JsRouteInput` 带 method/url/headers/body_base64；`JsRouteOutput` 带 status/headers/body_base64；body 经 base64 跨 JS 边界。                                                                                |
| `handle_js_worker_route`                      | 653   | 见下「JS Worker HTTP 路由管线」。                                                                                                                                                                                                     |
| `guess_mime_type`                             | 886   | 文件扩展名 → MIME 静态映射；默认 `application/octet-stream`。                                                                                                                                                                       |
| `serve_static_file`                           | 916   | 仅 GET/HEAD（否则 405 + `ALLOW: GET, HEAD, OPTIONS`）；空/`/` 路径 → `index.html`；`resolve_safe_file_path`（traversal 错误 400）；NotFound 目录回退 `<resolved>/index.html`；ETag = `"\"{mtime_secs}-{len}\""`（weak）；处理 `If-None-Match`（`*` 或逗号分隔、去 `W/`）→ 304 不读文件；HEAD 空 body；可选 CORS `Access-Control-Allow-Origin: *`。 |
| `build_http_error / build_static_error`       | 1023  | `text/plain` 错误响应；后者可选加 CORS 头以让浏览器可读。                                                                                                                                                                            |
| `bind_unix_listener`                          | 1062  | `#[cfg(not(target_os="windows"))]`：创建父目录、移除已存在 socket（忽略 NotFound）、绑定 `UnixListener`。                                                                                                                                  |
| `cleanup_unix_socket_file`                    | 1084  | shutdown 时移除 socket 文件；忽略 NotFound，其他警告。                                                                                                                                                                                |
| `ServerPermissionChecker impl PermissionChecker` | 1104 | 零大小 struct 实现 `ng-core::PermissionChecker`；`check_token_limit`/`check_super_token`/`get_token` 各返回 `Box::pin` 包装对应 `ng_token` 函数，借用输入 lifetime `'a`（无 Vec clone）。                                                            |
| `TaskMonitoringUuidProvider impl MonitoringUuidProvider` | 1147 | 委托 `MonitoringUuidCache::global().expect(...).get_or_insert(uuid)` 与 `MonitoringUuidCache::reload()`。                                                                                                                             |
| `JsWorkerServiceImpl impl JsWorkerService`    | 1176  | `run_inline_call_and_record_result` 委托 `ng_js_worker::service`；`get_rpc_module` 返回 `Box<dyn RawJsonDispatcher>` 包装 `RpcModuleDispatcher(get_modules())`。                                                                                  |
| `RpcModuleDispatcher impl RawJsonDispatcher`  | 1219  | 持有 cloned `RpcModule<()>`；`raw_json_request` clone JSON 字符串与 module，调用 `module.raw_json_request(json, buf_size)`，返回 `(resp.to_string(), ())`。让 JS 脚本可以通过 raw JSON 调用任意 server RPC。                                            |
| `CronJsWorkerScheduler impl JsWorkerScheduler` | 1244 | `enqueue_run` 委托 `ng_js_worker::service::enqueue_defined_js_worker_run(worker_name, run_type, params, env_override)`，返回 `i64`。                                                                                                    |

### build.rs — `server/build.rs:1`

- 当 `CARGO_CFG_TARGET_ARCH == "arm"` 时，emit `cargo:rustc-link-arg=-latomic`，让 QuickJS 的 64-bit 原子调用（`__atomic_*_8`）在链接期解析；该链接参数位于链接命令的**末尾**，在 `.rlib` 之后。其他架构 no-op。

## 内部机制

### 启动顺序与 runtime

`main()` 构建多线程 Tokio runtime（`global_queue_interval(3)`）并 `block_on(async_main)`。`async_main` **先**初始化 `ng_js_runtime`（确保 JS 全局对象在任何 crate 使用前存在），再解析参数、处理 Version 短路、设置 config path + reload notify、解析配置、初始化日志、写全局配置，最后分发。Serve 分支进入 `loop { serve::run(&config).await; re-parse; set_server_config; reload_static_path; clear_dav_handler_cache; config = reloaded }`，`serve::run` **仅在 shutdown 或 reload 信号时返回**，每次迭代即一个服务端生命周期。

### 热重载生命周期

`serve::run` biased `select` 在 `serve_future` 完成与 `get_reload_notify().notified()` 之间。shutdown 时：flush `ng_monitoring::monitoring_buffer`、`DbRegistryManager.shutdown`、5s 超时 `stop_handle.shutdown()`、abort Unix socket 任务、移除 socket 文件。reload 时执行同样步骤，但 `stop_handle.shutdown()` **非阻塞 spawn**，使 `run()` 立即返回，让 `main` 的循环重新解析配置并重启。

### Trait 注入接线

`serve::run` 在构建 RPC 模块**之前**注册 `ServerPermissionChecker`（ng-core）、`TaskMonitoringUuidProvider`（ng-task）、`JsWorkerServiceImpl`（ng-js-runtime）、`CronJsWorkerScheduler`（ng-crontab），全部委托到 `ng_token` / `ng_monitoring` / `ng_js_worker` 函数。`RpcModuleDispatcher` 让 JS 能经 raw JSON 调用任意 server RPC（通过 `get_modules()`）。

### RPC 模块组装与缓存

`build_modules` 在首次 `get_modules()` 调用时把 **9** 个 `RpcModule` merge 进一个空 `RpcModule<()>`，然后缓存在进程级 `static OnceLock<RpcModule<()>>`。其中 `ng_monitoring::rpc_module()` 自身已先合并 `agent`、`agent-uuid` 与 `nodeget-server::list_all_agent_uuid` 三部分。之后每次调用 clone（廉价）。此路径被 `JsWorkerServiceImpl::get_rpc_module` 在**每次** inline JS RPC 分发时重入。

### Stream log 广播路径

订阅者用 `ArcSwap<HashMap>` + `AtomicUsize` 快路径计数。读（`on_event`）先做一次 `has_subscribers()` 检查，再 `load()` 一个 `Arc` 快照、过滤感兴趣的 Sender、把事件序列化**一次**、`tx.try_send()` 预序列化 JSON 字符串给每个订阅者（非阻塞，满则丢弃，避免慢订阅者阻塞 logging 线程）。写（add/remove）用 `rcu()` 整体交换 map。设计目标是规避之前 `std::sync::RwLock` 在 `on_event`（读）与 `add/remove`（写）之间的死锁。

### 内存日志环

`JsonFieldVisitor` + `build_log_entry` 被 `MemoryLogLayer` 与 `JsonRemapFormat` 共享，两者均用直接 `Map` 构造以避开 `serde_json::json!` 的中间分配。`StreamLogLayer` 仍使用 `serde_json::json!`。`FormattedFields` 的 ANSI 码在进入 JSON/memory/stream 输出前经 `strip_ansi` 剥离。

### HTTP 请求路由

`/`、`/nodeget/rpc`、fallback 三处的分支逻辑：WS upgrade → `jsonrpc_service.call`；GET 且 `StaticCache.get_http_root()` 启用 → `serve_static_file`；否则 `landing_html`（仅当无 cache 或 http-root 被禁用时）或 `jsonrpc_service.call`。非 WS 的 RPC 错误经 `unwrap_or_else` 捕获为 500 响应 + 记录日志，**刻意避免**在 handler task 里 `unwrap()` panic。ETag 弱校验使用 `mtime_secs-len`；`If-None-Match` 匹配返回 304 且**不读文件**。

### JS Worker HTTP 路由管线

`handle_js_worker_route` 是 JS Worker HTTP 入口：按 RouteName 查 `js_worker` Entity、`ensure_bytecode_version` 保证字节码是最新的、用 `RunType::Route` + `RuntimeLimits::from_model` 经 `runtime_pool` 执行，**只**转发白名单响应头（`content-type`、`content-length`、`cache-control`、`last-modified`、`etag`、`access-control-allow-origin`/`-methods`/`-headers`）以防止 JS 注入敏感头（如 `Set-Cookie`/`Location`/CSP）。body 经 base64 跨 JS 边界；请求 body 上限 **`ROUTE_BODY_LIMIT_BYTES = 8 MiB`**（`axum::body::to_bytes`）。会剥离客户端伪造的 `ng-connecting-ip` 并注入 `ng-connecting-ip=peer_ip`；URL 重建时尊重 `x-forwarded-proto` 与 `Host`；peer IP 默认 `127.0.0.1`；`route_name` trim 后为空返回 400；DB 错误 500、未找到 404、输出反序列化失败 500。

### self_update 二进制替换流程

`reqwest` 下载（**2 分钟**超时、`User-Agent: NodeGet-Server`），拒绝小于 **1024 字节**的响应，调用 `ng_core::self_update::replace_binary`，Unix 上对规范化 exe 路径 `chmod 0o755`（失败仅 warn），然后 spawn 一个 **3 秒**延迟的重启任务（Windows 用 `restart_process`，其他用 `restart_process_with_exec_v`），使 RPC 的 Ok 响应能在进程被替换之前送出。先 `check_if_update_needed(tag)`，已在目标版本则直接 Ok 返回。

### TLS / ALPN

`build_http1_only_tls_config` 强制 ALPN 为 `[http/1.1]` 以避开 HTTP/2 协商开销。只有 TCP+TLS 路径；WS/JSON-RPC 协议仅 HTTP/1.1。

### build.rs ARM libatomic

32-bit ARM 目标 emit `-latomic`，因 QuickJS 发出的 `__atomic_*_8` 调用位于 libatomic；该链接参数置于链接命令末尾，在 `.rlib` 之后解析。

## RPC 方法

命名空间 `nodeget-server`（`_` 分隔符）。每个 handler 用 `info_span!(target:"server", "nodeget-server::<method>", token_key, username)` 包装；`token_identity(&token)` 在 token 被 move 之前抽出 `(token_key, username)` 作为 span 字段。

| 方法                 | 参数                                            | 所需权限                                                                                            | 行为                                                                                                                                               |
|----------------------|-------------------------------------------------|-----------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------|
| `hello`              | none                                            | 无（开放）                                                                                           | 返回字面量 `'NodeGet Server Is Running!'`。                                                                                                         |
| `version`            | none                                            | 无（开放）                                                                                           | 返回 `NodeGetVersion::get()` 序列化的 JSON `Value`（语义版本 + 构建信息）。                                                                          |
| `uuid`               | none                                            | 无（开放）                                                                                           | 返回全局配置的 `server_uuid`；配置缺失/锁中毒返回空字符串。                                                                                          |
| `read_config`        | `token: String`                                 | **Super Token**（由 `ng_config::server_rpc` 下游校验）                                               | 委托 `ng_config::server_rpc::read_config`，读取 server 配置文件内容为 TOML 字符串。                                                                  |
| `edit_config`        | `token: String, config_string: String`          | **Super Token**（由 `ng_config::server_rpc` 下游校验）                                               | 委托 `ng_config::server_rpc::edit_config`，写入 TOML 并触发热重载（发送 `RELOAD_NOTIFY`）；返回 `bool`。                                              |
| `database_storage`   | `token: String`                                 | **Super Token**（由 `ng_db::rpc::nodeget::database_storage` 下游校验）                               | 经 `rpc_exec!` 委托 `ng_db::rpc::nodeget::database_storage`，报告 DB 各表的尺寸/行数。                                                               |
| `log`                | `token: String`                                 | **Super Token**（解析 `TokenOrAuth` + `check_super_token`，否则 `PermissionDenied`）                  | 返回内存环快照（`logging::get_memory_logs()`）为 JSON `RawValue`。                                                                                  |
| `stream_log`         | `token: String, log_filter: String`（subscription；unsubscribe：`unsubscribe_stream_log`） | **Super Token**                                                                                      | 解析 token，失败以 ErrorObject code **101/102** `reject()` 后返回 `Ok(())`；成功后 accept sink，注册 `Uuid` 订阅者与 per-subscriber `StreamFilter`（EnvFilter 兼容，如 `info,server=debug`），将每条匹配事件以 JSON `Value` 转发直到客户端断开。 |
| `list_all_agent_uuid`| `token: String`                                 | Super Token；或具备 `NodeGet::ListAllAgentUuid` / `MonitoringUuid::List` 且满足 scope 过滤的 Token    | 由合并进来的 `ng_monitoring` `nodeget-server` 子模块提供，返回 `{"uuids": [...]}`；全局权限可见全部 UUID，`AgentUuid` 作用域时返回按权限过滤后的子集。 |
| `self_update`        | `token: String, tag: String`                    | **Super Token**                                                                                      | 下载 `tag` 对应 release（已在目标版本则跳过），替换运行中二进制（Unix `chmod 0o755`），spawn 3s 延迟重启任务（见「self_update 流程」）。                |
| `exec_sql`           | `token: String, sql: String, params: Option<Value>` | `NodeGet::ExecSql`（`Scope::Global`；**文档化为完全信任**）                                           | 经 `rpc_exec!` 委托 `ng_db::rpc::nodeget::exec_sql`，在主 DB 上执行原始 SQL。**完全信任**（任意 SQL；SQLite 下 `ATTACH` 升级为 FS 读写）。 |
| `get_database_type`  | `token: String`                                 | `NodeGet::ExecSql`（`Scope::Global`）                                                                 | 经 `rpc_exec!` 委托 `ng_db::rpc::nodeget::get_database_type`，返回 `sqlite` / `postgres` / `mysql` / `unknown`。                                    |

### 鉴权流程（命名空间级）

`log`、`stream_log`、`self_update` 在 handler 内自行解析 `TokenOrAuth::from_full_token` 并调用 `check_super_token`，非 super 返回 `PermissionDenied('Super token required')`（`stream_log` 以 ErrorObject code 101/102 `reject()` 到 `PendingSubscriptionSink`）。`read_config`/`edit_config` 委托 `ng_config` 下游做 super-token 校验；`database_storage` 委托 `ng_db` 下游做 super-token 校验；`exec_sql` / `get_database_type` 委托 `ng_db` 下游检查 `Scope::Global + Permission::NodeGet(NodeGet::ExecSql)`；`list_all_agent_uuid` 委托 `ng_monitoring` 下游根据 super token / 列表权限与 scope 过滤可见 UUID。`hello`/`version`/`uuid` 完全开放。

## Crate 内部约定

- **Edition 2024**：在 `serve.rs` fallback 与 `handle_js_worker_route` 头部构造中使用 let-chains（`if let .. && let ..`）。
- **Feature gates**：依赖所有 `ng-*` crate 并启用其 `server` feature；`dhat-heap` 可选 feature 切换全局分配器为 `dhat::Alloc` 做堆 profile。
- **RPC 宏纪律**：仅使用 `#[rpc(server, namespace=...)]` + `#[method(name=...)]` + `#[subscription(...)]`，**绝不**手动 `register_method`。命名空间分隔符为 `_`（自定义 jsonrpsee fork）。
- **统一返回类型**：所有 RPC handler 返回 `RpcResult<Box<RawValue>>`（简单的 `String`/`Value`/`()` 会自动转换）；委托经 `rpc_exec!` 宏统一日志与错误转换。
- **日志 target**：server binary 侧的核心 target 有 `"server"`（server 内部 span）、`"rpc"`（timing 中间件）、`"js_worker"`（JS 路由处理）、`"db"`（DB 层事件，虚拟别名，实际展开为 `sea_orm*`/`sqlx*`）；合并进来的业务 crate 还会使用各自的 target（如 `crontab`、`terminal`、`monitoring`、`static` 等）。
- **凭据绝不走 tracing**：Super Token、Root Password 一律 `println!` 到 stdout，因为 tracing 会进入内存日志缓冲与 JSON 日志文件（可经 RPC 查询）。
- **内联默认值**：超时 3000ms / `max_lifetime` 30000ms / `max_connections` 10（`init_db_connection`）；`max_conns` 100 / `resp` 100MiB / `req` 10MiB（jsonrpc server）；monitoring `db_path` 默认 `./db/`；Unix socket 默认 `/var/lib/nodeget.sock`。
- **`OnceLock` 全局单例**：`MEMORY_LOG_BUFFER`、`MEMORY_LOG_CAPACITY`、`STREAM_LOG_MANAGER`、`GLOBAL_RPC_MODULE`，以及各 crate 的 setter 设置的 trait 提供者。
- **tracing 字段卫生**：timing 中间件把 request id 作为字段内联，避免每请求 `String` 分配；visitor 直接构建 `serde_json::Map`（不走 `json!` 宏）用于热日志路径。
- **中文注释**：内联注释与 doc 注释均为中文，编辑时保持一致。

## 注意事项与陷阱

- **维护者必须意识到 `server/src/main.rs:101` 的 Serve 分支是无限循环、无退出条件、无重试上限、无 backoff**。`serve::run` 仅在 shutdown 或 reload 信号时返回；一个不触发 reload 的真正致命错误会让循环空转，每轮重新解析配置。
- **切勿假设配置热重载解析失败会停服**：`server/src/main.rs:117`（及 `:126` 的 `set_server_config` 失败）失败时 `continue`，立即以**旧** config 再次 `serve::run`。若坏配置持续存在，会无延迟热循环。
- **`server/src/rpc_nodeget.rs:356` 的 `self_update` 是远程代码执行路径**：URL 由用户提供的 `tag` 派生，下载并替换运行中的 server 可执行文件再 exec 新进程。虽 super-token 守卫，下载源（`ng_core::self_update::get_server_url`）与签名模型必须可信；release 源被攻破或下载链路上的 DNS/网络 MITM 即等于攻破 server。
- **`server/src/rpc_nodeget.rs:198` 的 `exec_sql` 是完全信任 RPC**，在主 DB 上执行任意 SQL（委托 `ng_db::rpc::nodeget::exec_sql`）；SQLite 下 `ATTACH DATABASE` 升级为 server uid 下的任意 FS 读写。CLAUDE.md 文档化为**有意为之**；**切勿**放宽其权限，并在最小权限 uid 下运行 server。
- **`server/src/subcommands/serve.rs:422`（TLS 分支）与 `:474`（明文分支）的 `select` 两侧都对 `serve_future` 结果 `unwrap()`**：若 `axum::serve` 中途返回 Err（如监听器失败），server 进程在 shutdown 期间 `panic`，外层 `main` 循环随后重启。属刻意 fail-fast，但确是硬 panic。
- **`server/src/subcommands/serve.rs:121`：`RpcTimingMiddleware` 硬编码为 `tracing::Level::TRACE`**。RPC 计时日志在 TRACE 级别发出，运维若要看到须把 `rpc` target 设为 `trace`（或全局 `trace`），日志量很大，预期容易错位。
- **`server/src/subcommands/serve.rs:402`：`config.ws_listener` 用 `parse().unwrap_or_else(panic)` 解析**。`config.toml` 中监听地址格式错误会在启动时直接 panic，无回退。
- **`server/src/logging/mod.rs:134`：`memory_log_capacity = 0` 会禁用内存日志层**（`MemoryLogLayer` 不安装），但 `MEMORY_LOG_CAPACITY` 仍被设为 0，`get_memory_logs()` 返回空。`nodeget-server_log` RPC 会静默返回 `[]`，**不**报错。
- **`server/src/logging/mod.rs:768`：`StreamLogFilter` 必须作为 per-layer filter（`.with_filter`）使用，绝不可作为全局 subscriber filter**。在 Layered AND 逻辑下，全局 filter 在无订阅者时返回 false 会阻塞**所有**其他层（console、memory、json）。任何重构 subscriber init 的人都必须保留这一点。
- **`server/src/subcommands/serve.rs:657`：JS Worker HTTP 路由请求 body 上限 `ROUTE_BODY_LIMIT_BYTES = 8 MiB`（超出 400），但响应 body 无上限**——返回超大 base64 body 的 JS worker 会完整缓冲进内存后才响应，可经此 OOM server。
- **`server/src/subcommands/serve.rs:857`：`handle_js_worker_route` 只转发 `ALLOWED_RESPONSE_HEADERS` 白名单**。JS worker 设置的任何白名单外头部（自定义 `X-`、或被刻意阻断的 `Set-Cookie`/`Location`）会被静默丢弃，依赖这些头部的 worker 不会有清晰错误，看起来像是「坏掉」。
- **`server/src/subcommands/serve.rs:1114`：`ServerPermissionChecker::check_token_limit` 返回的 future 借用调用方的 `TokenOrAuth`/scopes/permissions（lifetime `'a`）**。调用方必须把它们存活到 awaited future 完成（提前 drop 是编译期借用错误）。设计如此（避免 Vec clone），但约束了 `ng-db`/`ng-kv`/`ng-static` 等的调用模式。
- **`server/src/rpc_nodeget.rs:517`：`get_modules()` 在进程级 `OnceLock` 缓存单个合并 `RpcModule` 并 clone**。该模块被共享（含 `JsWorkerServiceImpl` 的 JS→RPC 分发），**切勿**在 module context（类型为 `()`）里存每请求状态。缓存**永不**在热重载间失效——合并的命名空间只反映构建期链接的 crate。
- **`server/src/main.rs:62`：`Version` 是特殊的提前返回子命令**，在 `async_main` 中于配置解析与日志初始化**之前**短路（`:65`）。新增到 Version 分支的代码不得依赖配置/日志就绪；`:145` 还有一个重复 arm（为完整性），但因提前返回而不可达。
- **`server/src/subcommands/mod.rs:33`：`init_or_skip_super_token` 与 `roll_super_token` 把 Super Token + Root Password 打印到 stdout，正是为了避开 tracing**。任何对这些值做结构化 tracing 的人都会把凭据泄漏进可经 `nodeget-server_log` RPC 查询的内存日志缓冲与 JSON 日志文件。**绝不可** trace 凭据。
- **`server/src/subcommands/serve.rs:407`：TLS 当且仅当 `tls_cert` 与 `tls_key` 同时为 `Some` 时启用**。只设置其一会**静默回退到明文 TCP**——一个会泄漏未加密流量的可能误配；仅设置一个时没有 warn。
- **`server/build.rs:8`：`build.rs` 仅在 `CARGO_CFG_TARGET_ARCH == "arm"` 时 emit `-latomic`**。其他同样缺 64-bit 原子的 32-bit 非 ARM 目标（如部分 RISC-V 或嵌入式）**不会**得到 `-latomic`，可能链接失败。该检查刻意狭窄，但交叉编译时值得知晓。
- **`server/src/logging/mod.rs:873`：`StreamLogLayer.on_event` 用 `try_send`，订阅者 512-capacity `mpsc` 满时静默丢弃日志条目（`let _ = tx.try_send(...)`）**。慢的 `stream_log` 订阅者会无任何迹象地丢日志行；容量 **512** 在 `rpc_nodeget.rs::stream_log` 中设置。

## 依赖关系

`nodeget-server` 是组合根，依赖工作区内所有业务 crate（`ng-core`、`ng-db`、`ng-infra`、`ng-config`、`ng-monitoring`、`ng-token`、`ng-kv`、`ng-task`、`ng-crontab`、`ng-js-runtime`、`ng-js-worker`、`ng-static`、`ng-terminal`），并启用它们的 `server` feature。它还引入 `axum` / `axum-server` / `rustls` / `tokio` / `tracing` / `tracing-subscriber` / `reqwest` / `arc-swap` 等外部 crate。它是最终可执行产物，不被任何其他 workspace crate 依赖；agent 二进制与之并行，二者通过 WebSocket + JSON-RPC 在运行时通信。
