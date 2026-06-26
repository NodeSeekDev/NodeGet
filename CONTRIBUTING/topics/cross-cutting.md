# 跨 Crate 机制

> 本文详述贯穿多个 crate 的横向机制：Trait 注入、RBAC 权限模型、缓存框架、安全边界。这些是系统的不变量骨架，改动前必须理解全文。

## 1. OnceLock Trait 注入（打破循环依赖）

业务 crate 之间存在循环依赖（如 ng-task 需要 ng-token 鉴权，ng-token 又依赖 ng-infra 的 RPC 设施）。NodeGet 用 **OnceLock + trait object** 在运行时注入实现，编译期只依赖 trait 定义（在 `ng-core` 或下层 crate）。

### 注入点全景

| 注入函数 | 定义 Crate | 用途 | server 实现 |
|----------|-----------|------|------------|
| `set_auth_checker` | ng-infra | 认证 → Token 元数据 | server binary |
| `set_token_checker` | ng-kv / ng-static / ng-terminal / ng-js-worker | 各命名空间权限 | server binary（委托 ng-token） |
| `set_auth_provider` | ng-db / ng-task | db / task 命名空间权限 | server binary |
| `set_monitoring_uuid_provider` | ng-task | UUID 缓存访问 | `TaskMonitoringUuidProvider` |
| `set_check_super_token_fn` | ng-config | config RPC super-token 校验 | server binary |
| `set_js_worker_service` | ng-js-runtime | JS 执行调度（内联调用 + rpc module） | `JsWorkerServiceImpl` |
| `set_js_worker_scheduler` | ng-crontab | cron → JS worker 调度 | `CronJsWorkerScheduler` |
| `require_permission_checker` | ng-core（`permission_checker.rs`，for-server） | 对象安全 async trait，替代原先 6 个分散 auth trait | `ServerPermissionChecker` |

### 约定

- `set_*` 静默忽略重复初始化：`let _ = LOCK.set(val);`。
- `get_*` / `require_*` 未初始化时 **panic**（`.expect("... not initialized -- call set_* first")`）——这是 fail-fast，比静默返回错误更安全（启动接线错误会立刻暴露）。
- 部分函数返回 `Option`（如 `ng_db::get_db()`）而非 panic——这些是可选设施。
- **所有 server 实现最终委托给 `ng_token` 函数**——ng-token 是权限的单一事实源。
- server binary 在 `serve.rs` **集中注册**，顺序敏感（缓存 init 依赖 trait 注入完成）。

## 2. RBAC 权限模型

### 凭据：`TokenOrAuth`

`TokenOrAuth`（`ng-core/src/permission/token_auth.rs`）支持双模认证：

- **Token 模式**：`key:secret`（`:` 分隔）。
- **Auth 模式**：`username|password`（`|` 分隔）。

**分隔符优先级是 load-bearing**：`from_full_token` 先按 `:` 切（Token 优先），再按 `|` 切。`a:b|c` 解析为 Token 模式（key=`a`）。`extract_target_identifier`（ng-token `rpc/utils.rs:23`）遵循同样优先级。

**空半边被接受**。用户名**禁用** `:` 与 `|`（创建时 fail-fast 检查，`generate_token.rs:77`）——否则会破坏分隔符语义。

### Token / Limit / Scope / Permission

- `Token`（`permission/data_structure.rs`）：`id`、`token_key`、`secret_digest: [u8;32]`（SHA256 + "NODEGET" 盐）、`token_limit: Arc<Vec<Limit>>`、时间窗 `timestamp_from/to`、`is_super`。
- `Limit { scope: Scope, permissions: Vec<Permission> }`：一条 limit 约束一组 scope × permission。
- `Scope` 枚举：`Global` / `AgentUuid` / `KvNamespace` / `JsWorker` / `StaticBucket` / `Db`（serde `snake_case`）。
- `Permission` 枚举：按领域分组（`NodeGet`、`Kv`、`Token`、`JsWorker`、`JsResult`、`StaticBucket`、`StaticBucketFile`、`Crontab`、`CrontabResult`、`Db`、`Agent`、`Task`），各携子操作枚举。serde `snake_case`。

### 鉴权语义（关键不变量）

- **`check_token_limit` 是 AND 语义**（ng-token `get.rs:377`）：对 `scopes × permissions` 的**全笛卡尔积**要求**每一对**都被某条 limit 覆盖。误以为 OR 语义会导致权限收紧（而非放宽），但仍是 bug 源——传长度不匹配的切片会得到意外结果。
- **通配符仅后缀**（`wildcard_matches_pattern`，`get.rs:189`）：只把**尾部** `*` 当通配；`a*b` 按精确匹配，中缀 `*` 被当字面量。
- **super-token 绕过所有限制**：id=1，`check_super_token` 用常量时间比较（`subtle`）。super 经独立路径认证（`cache.rs:282-314`），不计 limit。
- **时间窗校验**：`check_token_limit` 内部检查 `timestamp_from/to`；列表过滤快速路径用 `token_time_valid` 补齐（fail-closed：时间读取失败返回 false）。

### 鉴权两种实现路径

- **单次操作**：走完整 `check_token_limit`（含 `get_token` 的 ct_eq 验证）。
- **批量列表**（如 `list_all_*`）：先短路 super-token，否则 `get_token` 一次拿 `Arc<Vec<Limit>>` 切片，对每项用 `check_limits_cover` 纯内存匹配——**避免 N 次串行 `check_token_limit`**（每次重复 `get_token` + clone limits）。

### 哈希与盐

- SHA256 + 硬编码盐 `b"NODEGET"`（`ng-token/src/lib.rs:70`）。
- **无 per-entry salt**：相同 secret 产生相同 hash（彩虹表友好）。常量时间比较仅防时序，不防预计算。
- **改动盐值必须重新哈希所有现存凭据**——这是破坏性操作。
- hex 解码失败时 `build_maps` 回退 `[0u8;32]`（`cache.rs:194`）；实际 SHA256 不会产生全零。

## 3. 缓存框架

### `DbBackedCache` trait + `make_global_cache!` 宏（ng-infra `server.rs`）

全量 DB 加载缓存的统一设施。实现 trait 后用宏生成 `OnceLock` 全局单例，自动获得 `init()` / `global() -> Option<&'static Self>` / `reload()`。

**宏的内部约定**（`make_global_cache!`）：

- 生成的 static 是 `OnceLock<CACHE_TYPE>`（**不是** `OnceLock<Option<Arc<...>>>`）。
- `init()` 无参，内部调 `load_all()`。
- `global()` 返回 `Option<&'static Self>`。
- `reload()` 是 async，重新 `load_all()` 后调 `reload_from_models(&self, models)`（注意 `&self`，非 `&mut self`——内部用内部可变性）。
- `init()` 用 `set().is_err()` 回退到 `reload()`（已初始化则刷新）。
- 宏内标识符加 `__` 前缀避免卫生冲突；trait 路径用 `$crate::server::DbBackedCache` 保证卫生。

现有 DB-backed 缓存：`TokenCache`、`CrontabCache`、`StaticCache`、`MonitoringUuidCache`。

### `RpcHelper` 与 `rpc_exec!`

- `RpcHelper`（ng-infra `server.rs`）：blanket `impl RpcHelper for ...`，为 RPC impl struct 提供 `into_rpc()` 之外的通用方法（错误映射辅助）。
- `rpc_exec!` 宏：包裹 handler 调用，统一注入 tracing target `rpc`、统一错误桥接（`anyhow_error_to_raw`）。
- `token_identity(&token) -> (token_key, username)`：从凭据提取结构化字段供 `info_span!`，**避免明文 token 入日志**。
- `TruncatedRaw`：截断超长 RawValue 用于日志，用 `floor_char_boundary` 保证不截断 UTF-8。

### 派生态内存缓存（非宏）

`MonitoringLastCache`、`StaticHashCache` 持有派生/最近值，**手写** `static CACHE: OnceLock<...>` 单例，不用 `DbBackedCache`。

## 4. 安全边界

### ExecSql（有意为之的全信任）

`NodeGet::ExecSql` 在主库跑任意 SQL。SQLite 后端下 `ATTACH DATABASE 'any/path'` 升级为**任意文件读写**（server uid 下，可创建/覆盖文件、读其他 `.db` 文件、绕过 `db_registry` 路径约束）。这是文档化特性，**仅授予完全受信运维者**，并以最小权限 uid 运行 server。

### 路径安全

静态文件操作三件套（`ng-static`）：

- `validate_name`：bucket/file 名字符集白名单。
- `validate_sub_path`：子路径校验。
- `resolve_safe_file_path`：规范化后 canonicalize 检查，防 `..` 遍历。
- `normalize_route_name`（ng-js-worker `route_name.rs`）：`[a-zA-Z0-9._-]` + 长度 ≤128 + 拒绝纯点组合（`.` / `..`）。

**任何新路径处理代码必须遵守同样纪律**——这是 HTTP 路由路径遍历的最后防线。

### WebSocket 大小限制

- terminal WS：帧 ≤1MB、消息 ≤4MB，超限拒绝。
- agent ↔ server 通信：broadcast 通道 cap 32（agent `multi_server.rs`）。

### 凭据不入 tracing

server 子命令输出凭据（如 `roll_super_token`）用 `println!`，**不**用 tracing——tracing 会落盘/聚合到日志系统。

### super-token 检测

`generate_super_token` 用 `sql_err() == UniqueConstraintViolation` 判定「已存在」（`super_token.rs:66`），以存活 PG 中文 locale。**切勿**改为错误字符串匹配——若 backend/SeaORM 版本不暴露该 enum 变体，会返回 `DatabaseError` 而非 `None`，破坏幂等。

## 5. 配置热重载

- 两侧都监听 `RELOAD_NOTIFY`（`Notify`，`ng-config`）。
- **server**：重新读 config 文件 → reload 所有 `DbBackedCache` → 重启 loop（**不**重新注入 trait——trait 实现是启动期一次性）。
- **agent**：收到 `EditConfig` 任务 → 成功后 `RELOAD_NOTIFY.notify_one()` → abort 所有 handle → 重新 loop。
- **agent NTP 不重取**：NTP offset 仅进程启动时获取一次（`NTP_INIT_DONE` guard，`main.rs:51`）。热重载**故意**不重取 NTP，避免 offset 跳变。新增重取路径需谨慎 gating。

## 6. 监控数据缩放约定

ng-monitoring 的动态摘要字段用 `i16` 存储缩放后的值（百分比/千分比），伴有 NaN 检查与 clamp。`SCALED_SUMMARY_COLUMNS` 枚举成员身份决定哪些列走缩放路径。`canonical-hash` 用 `WriteToDigest` adapter 计算确定性哈希。详见 [`crates/ng-monitoring.md`](../crates/ng-monitoring.md)。

## 7. 任务系统约定

- **`task.query` 默认上限 1000 行**（`DEFAULT_LIMIT`），需要更多的客户端必须显式指定 `Limit` 条件。`MAX_LIMIT` 上限通常 10000（防返回/删除过多数据）。
- **`TaskEventResponse` 三级回退**保证总有 ack：完整序列化 → 最小错误 ack → 手工 `json!()`。重构必须保留「永远 ack」属性（agent `tasks/mod.rs:519`）。
- **任务执行绝不退出进程**：`self_update` 失败返回 `false`（`agent/src/tasks/self_update.rs:26`），**不**调 `exit(1)`——否则一个坏任务会杀掉整个 agent。
- **Unix 子进程用 `process_group(0)`**（`agent/src/tasks/execute.rs:59`）：pgid == child pid，超时 `libc::killpg` 能收割孙进程（shell fork 的子进程）。移除会导致超时时孙进程孤儿化。
