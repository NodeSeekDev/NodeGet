# 系统架构与数据流

> 本文是 NodeGet 后端的架构总览，跨越所有 crate。维护者在做架构级修改前应先读本文。各组件的实现细节见 [`crates/<name>.md`](../crates/)。

## 1. 总体拓扑

NodeGet 是一个 **WebSocket + JSON-RPC 2.0** 的监控与自动化系统，由两个二进制组成：

```
┌─────────────────────┐         WebSocket (JSON-RPC 2.0)         ┌─────────────────────┐
│   nodeget-agent     │  ◄──────────────────────────────────►    │   nodeget-server    │
│  (N 个 server 并发) │        agent = WS client；server = WS srv │  (HTTP + WS at /)   │
└─────────────────────┘                                          └─────────┬───────────┘
       │                                                                  │
       │ 采集静态/动态监控数据                                              │ 组装 14 个 RPC 命名空间
       │ 执行下发任务（ping/dns/http/exec/pty/...）                        │ 注入 trait 实现
       │                                                                   │
       └─── RPC 上行：agent_* / task_upload_task_result                   └─── PostgreSQL / SQLite (SeaORM)
            RPC 下行：task_register_task (push)                                全局单例 ng_db::get_db()
```

- **通信方向**：agent 是 WebSocket **客户端**，主动连接 server。一个 agent 可同时连接 N 个 server（多服务器）。
- **传输**：HTTP/1.1 + WebSocket（axum 0.8）。TLS 可选（rustls + aws-lc-rs）。Terminal、JS worker HTTP 路由复用同一 axum `Router`。
- **协议**：JSON-RPC 2.0，使用**自定义 jsonrpsee fork**（`infinitefield/jsonrpsee`），命名空间分隔符是 `_` 而非 `.`。因此 `nodeget-server_uuid` 是一个完整方法名（命名空间 `nodeget-server` + 方法 `uuid`）。

## 2. Workspace 组合

server binary 是**组合根（composition root）**，在 `server/src/rpc_nodeget.rs::build_modules()` 合并来自 8 个 crate 的 `RpcModule`，在 `server/src/subcommands/serve.rs` 注册所有 trait 实现。详见 [`binaries/nodeget-server.md`](../binaries/nodeget-server.md)。

### RPC 命名空间来源

| 命名空间 | 提供方 crate | 方法数 | 详见 |
|----------|--------------|--------|------|
| `nodeget-server` | server + ng-monitoring + ng-db + ng-config | 12 | [server](../binaries/nodeget-server.md) |
| `agent` | ng-monitoring | 11 | [ng-monitoring](../crates/ng-monitoring.md) |
| `agent-uuid` | ng-monitoring | 3 | [ng-monitoring](../crates/ng-monitoring.md) |
| `task` | ng-task | 6 | [ng-task](../crates/ng-task.md) |
| `token` | ng-token | 7 | [ng-token](../crates/ng-token.md) |
| `kv` | ng-kv | 8 | [ng-kv](../crates/ng-kv.md) |
| `db` | ng-db | 9 | [ng-db](../crates/ng-db.md) |
| `js-worker` | ng-js-worker | 7 | [ng-js-worker](../crates/ng-js-worker.md) |
| `js-result` | ng-js-worker | 2 | [ng-js-worker](../crates/ng-js-worker.md) |
| `crontab` | ng-crontab | 6 | [ng-crontab](../crates/ng-crontab.md) |
| `crontab-result` | ng-crontab | 2 | [ng-crontab](../crates/ng-crontab.md) |
| `static-bucket` | ng-static | 6 | [ng-static](../crates/ng-static.md) |
| `static-bucket-file` | ng-static | 5 | [ng-static](../crates/ng-static.md) |

所有 RPC 方法统一返回 `RpcResult<Box<RawValue>>`，通过 `ng_infra::rpc_exec!` 宏实现统一日志与错误桥接。Terminal 走独立 WebSocket（非 JSON-RPC）。

### Crate 依赖图

```
ng-core (error/version/utils/RBAC 数据结构/PermissionChecker trait)
  ↑
ng-db (SeaORM 实体/连接全局单例/DbRegistry/db RPC/migration)
  ↑
ng-infra (DbBackedCache + make_global_cache!、rpc_exec!、RpcHelper、token_identity)  ← server feature
  ↑
┌──────┬────────┬─────────┬─────────┬──────────┬──────────┐
ng-monitoring  ng-token  ng-kv   ng-task  ng-crontab  ng-static
   ↑              ↑                  ↑        ↑       (独立)
   │         ng-terminal       ng-js-runtime
   │              ↑                ↑
   │              │           ng-js-worker
   └──────────────┴────────────────┘
                   ↑
              server binary（组合根）
ng-config（独立，被 server/agent 直接引用）
```

- **循环依赖通过 OnceLock trait 注入打破**（见 [`topics/cross-cutting.md`](cross-cutting.md) 的「Trait 注入」）。
- Agent 仅依赖 `ng-core/for-agent` + `ng-config` + `ng-task` + `ng-monitoring`（均无 `server` feature）——保证 agent 不拉入 RPC/DB 代码。

## 3. 数据流

### 3.1 监控数据上行

```
agent 采集 (静态 5min / 动态+summary 1s 默认)
  → 序列化为 JSON
  → build_rpc_with_raw_data() 手工拼装 JSON-RPC id=1 文本帧
  → send_to(server_name, Message::Text)  [broadcast 通道 cap 32]
  → WebSocket 上行
  → server: agent_report_static / agent_report_dynamic / agent_report_dynamic_summary
  → MonitoringBuffer（mpsc → 批量 INSERT）
  → DB (static_monitoring / dynamic_monitoring / dynamic_monitoring_summary)
```

查询走内存缓存避免打 DB：`MonitoringUuidCache`（全量 DB 加载）、`MonitoringLastCache`（派生最近值，手写 OnceLock）、`StaticHashCache`（data_hash → 模型）。详见 [`crates/ng-monitoring.md`](../crates/ng-monitoring.md)。

### 3.2 任务下行

```
server: task_create / task_create_blocking RPC
  → TaskManager（broadcast 通道分发）
  → 推送给目标 agent（task_register_task 方法名复用为下行 push）
  → agent: handle_task 过滤 method=='task_register_task'
  → is_task_allowed 门控（server.allow_task_type 或 per-type allow_* 标志）
  → 执行（pool 路由网络任务 / WebShell 走会话信号量 / 其余 time::timeout(10min)）
  → 构造 TaskEventResponse（三级回退保证总有 ack）
  → task_upload_task_result 上行
  → server 写入 task 表
```

任务类型：`Ping/TcpPing/HttpPing/HttpRequest/WebShell/Execute/ReadConfig/EditConfig/Ip/Dns/Version/SelfUpdate`。详见 [`crates/ng-task.md`](../crates/ng-task.md) 与 [`binaries/nodeget-agent.md`](../binaries/nodeget-agent.md)。

### 3.3 JS Worker 执行

```
触发源（三选一）：
  - RPC: js-worker_run（CompileMode: Bytecode 走运行时池 / Source 走一次性 Runtime）
  - cron 调度: ng-crontab 通过 JsWorkerScheduler trait → enqueue_run
  - 内联调用: 另一 worker 内 inlineCall() → run_inline_call_and_record_result（同步等待）
  → 插入 js_result 行（start_time 已填）
  → QuickJS 执行（线程池常驻 / spawn_blocking 一次性）
  → 回填 js_result 行（finish_time + result | error_message，强制二选一）
```

inlineCall 递归深度上限 10（Rust 侧 `service.rs` + JS 侧 `server_runtime.rs` 双重检查）。详见 [`crates/ng-js-runtime.md`](../crates/ng-js-runtime.md) 与 [`crates/ng-js-worker.md`](../crates/ng-js-worker.md)。

## 4. 启动与热重载生命周期

### server（详见 [nodeget-server.md](../binaries/nodeget-server.md)）

```
main() → async_main()
  → 安装 rustls provider（幂等）
  → 解析 CLI（palc）、读 ServerConfig
  → init_logger（tracing-subscriber，json/fmt）
  → subcommand 分派：init / get_uuid / roll_super_token / serve
  → serve::run():
      init_db_connection()（SeaORM，SQLite 自动开 WAL）
      init_or_skip_super_token()（幂等生成 super-token id=1）
      ★ 注册所有 trait 实现（PermissionChecker / JsWorkerService / JsWorkerScheduler / MonitoringUuidProvider / 各 auth checker）
      ★ 初始化所有 DbBackedCache（Token / Crontab / Static / MonitoringUuid）
      build_modules() 合并 14 个 RPC 命名空间
      build Router（RPC + 静态 + WebDAV + terminal + worker-route）
      select! shutdown / RELOAD_NOTIFY
  → 热重载：收到 RELOAD_NOTIFY → 重新读 config → reload 所有缓存 → 重启 loop（不重新注入 trait）
```

### agent（详见 [nodeget-agent.md](../binaries/nodeget-agent.md)）

```
main()
  → 安装 rustls provider
  → 解析 AgentArgs、--version 分支
  → 读 AgentConfig
  → init_logger
  → 一次性获取 NTP offset（set_ntp_offset_ms，热重载不再重取）
  → dry_run()（仅 --dry-run，打印一轮采集数据后 exit 0）
  → init_connections（每 server 独立 connection_manager 协程，broadcast cap 32）
  → spawn 4 个服务循环：静态上报 / 动态上报 / 错误消息处理 / 任务处理
  → select! ctrl_c / RELOAD_NOTIFY
  → 热重载：RELOAD_NOTIFY（由 EditConfig 任务触发）→ abort 所有 handle → 重新 loop
```

**重连退避是两段式**：WebSocket 握手阶段（`connect_with_retry`）用指数退避 1s→2s→…→60s 上限 + ±20% jitter；已建立连接断开后，主循环固定 sleep 3s 再重试。

## 5. 数据库与存储

- **后端**：PostgreSQL 或 SQLite，通过 SeaORM 抽象。全局单例 `ng_db::get_db() -> Option<&'static DatabaseConnection>`。
- **SQLite 自动开 WAL**（`db_connection.rs`）。
- **19 步迁移**（`ng-db/migration`），实体由 `sea-orm-codegen` 生成到 `crates/ng-db/src/entity`。改 schema 流程：新增 migration → 跑迁移 → `sea-orm-cli generate entity`。详见 [`crates/ng-db.md`](../crates/ng-db.md)。
- **软删除**：`monitoring_uuid` 表用 `soft_delete` 标志而非真删；UUID 缓存在 `get_or_insert` 时自动复活软删条目。
- **DbRegistry**：额外的数据库注册表（多数据库支持），`has_conn(name)` 轻量存在性检查，避免克隆 `DatabaseConnection`。

## 6. HTTP 路由（非 RPC）

| 路径 | 处理者 | 来源 |
|------|--------|------|
| `/`, `/nodeget/rpc` | JSON-RPC + WebSocket + landing | server binary |
| `/nodeget/static/{name}/{*path}` | 静态文件服务 | `ng_static::router::router()` |
| `/nodeget/static-webdav/{*path}` | WebDAV（Basic Auth） | `ng_static::router::router()` |
| `/nodeget/worker-route/{name}/*` | JS worker HTTP 路由（新前缀） | server binary inline |
| `/worker-route/{name}/*` | JS worker HTTP 路由（legacy，过渡） | server binary inline |
| `/terminal` | Terminal WebSocket | `ng_terminal::router()` |
| `.fallback()` | WS upgrade / 静态根 / JSON-RPC | server binary |

## 7. 安全边界（高层）

- **RBAC**：每个 RPC 方法首参为凭据（`TokenOrAuth`：`key:secret` 或 `username|password`），handler 内部认证。Token 携带 `Vec<Limit>` 约束 scope+permission；check_token_limit 是 **AND 语义**（全笛卡尔积覆盖）。super-token（id=1，常量时间比较）绕过所有限制。详见 [`topics/cross-cutting.md`](cross-cutting.md) 的「RBAC 权限模型」。
- **ExecSql 是有意为之的全信任权限**：在主库上跑任意 SQL。SQLite 后端下 `ATTACH DATABASE 'any/path'` 可升级为任意文件读写（在 server uid 下）。这是文档化的特性，不是 bug——仅授予完全受信运维者，并以最小权限 uid 运行 server。
- **路径安全**：静态文件操作用 `validate_name`、`validate_sub_path`、`resolve_safe_file_path` 防遍历；route_name 用字符集白名单 + 拒绝纯点组合。任何新路径处理代码必须遵守同样纪律。
- **WebSocket 大小限制**：terminal WS 帧 ≤1MB、消息 ≤4MB，超限拒绝。
