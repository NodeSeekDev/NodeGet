# NodeGet 模块化重构计划

**目标**: 将 monolithic `server` + `nodeget-lib` 拆分为 14 个独立 crate，server binary 退化为薄入口。

**当前状态**: 3 crate workspace (server, agent, nodeget-lib)，server 含 ~80 个源文件，nodeget-lib 含 ~20 个源文件。

**分支**: 所有工作在 `dev-ref` 分支上进行，不污染 `dev` / `main`。

**版本**: 所有 crate（server, agent, ng-*）统一版本号 **0.5.0**，通过 `[workspace.package]` 继承。这是第一个支持模块化架构的版本。

**铁律 — 纯迁移，不改逻辑**: 所有代码搬迁必须原样搬移，不得修改任何 API 实现逻辑、函数签名、错误处理路径、缓存策略或业务行为。仅允许的变更：`use` 路径调整、模块声明调整、`pub` 可见性补齐。若搬迁中发现 bug，记录 Issue 单独修复，不在本重构中处理。

**铁律 — 最小化依赖**: 每个 crate 必须严格最小化第三方依赖，原则如下：

1. **`default-features = false`**：所有第三方依赖一律禁用默认 features，仅显式启用所需的 feature
2. **不引无用依赖**：逐 crate 审查每行 `use` 确认依赖必要性。types 层只用 `serde` 就不引 `serde_json`
3. **重依赖仅在 `server` feature**：jsonrpsee、sea-orm、dav-server、axum、rquickjs、llrt_* 等只能出现在 `server` feature 的 optional 依赖中
4. **Feature 粒度匹配**：依赖的 features 必须与实际使用匹配
5. **ng-agent-deps 零重依赖**：传递依赖链中不得出现任何 server 端依赖
6. **可用 `cargo tree -d` 检查**：每个 Phase 完成后用 `cargo tree -d --features server` 检查

---

## 全局单例策略

| 全局单例 | 归属 | 跨 crate 访问方式 |
|---------|------|-------------------|
| `DB: OnceCell<DatabaseConnection>` | ng-db | `ng_db::get_db() -> Option<&'static DatabaseConnection>` |
| `SERVER_CONFIG: OnceLock<RwLock<ServerConfig>>` | ng-config | `ng_config::get_server_config() -> Option<&'static RwLock<ServerConfig>>` |
| `RELOAD_NOTIFY: OnceLock<Notify>` | ng-config | `ng_config::get_reload_notify() -> Option<&'static Notify>` |

各业务 crate 通过调用上述函数获取引用，不再用 `crate::DB` 等路径。`update_global_config()` 留在 ng-config。

Agent 端全局单例 (`AGENT_ARGS`, `AGENT_CONFIG`, `NTP_INIT_DONE`) 留在 agent binary 内部不变。

---

## HTTP 路由组合策略

除 JSON-RPC 外，server 还暴露多种 axum HTTP 路由。拆 crate 后每个路由归属不同 crate，需要统一组合机制：

```rust
/// 每个提供 HTTP 路由的 crate 在 `server` feature 下暴露此函数
#[cfg(feature = "server")]
pub fn router() -> axum::Router;
```

| Crate | 路由 | 挂载路径 |
|-------|------|---------|
| ng-static | 静态文件服务 + WebDAV | `/nodeget/static/{name}` + WebDAV |
| ng-js-worker | JS Worker HTTP 路由 | `/worker-route/*` |
| ng-terminal | WebSocket 终端 | terminal WS upgrade |
| server binary | JSON-RPC 端点 + nodeget-server 自身路由 | `/` + `/nodeget/rpc` |

server binary 的 `serve.rs` 依次 `.merge()` 各 crate 的 `router()` 返回值。

---

## 权限类型归属（循环依赖解决方案）

**问题**: `PermissionResolver` 引用 `Token`/`Scope`/`Permission` 类型，但这些类型在 ng-token 中，而 ng-token 又依赖 ng-infra → 循环。

**解决**: 将纯数据结构下沉到 **ng-core**：

| 类型 | 原位置 | 新位置 | 理由 |
|------|--------|--------|------|
| `Token` (struct) | ng-token | **ng-core** | 纯 data struct + serde，无 DB/RPC 依赖 |
| `Limit` (enum) | ng-token | **ng-core** | 纯 enum |
| `Scope` (enum) | ng-token | **ng-core** | 纯 enum |
| `Permission` (enum) | ng-token | **ng-core** | 纯 enum |
| `TokenOrAuth` (enum) | ng-token | **ng-core** | 纯 enum |
| `TokenCache` | ng-token | ng-token | 有 DB 依赖 |
| `super_token::*` | ng-token | ng-token | 有 DB 依赖 |
| `generate_token` | ng-token | ng-token | 有 DB 依赖 |
| Token RPC | ng-token | ng-token | 有 jsonrpsee 依赖 |

ng-token 变为"权限模型的 server 端实现"（cache + super-token + RPC），ng-infra 的 `PermissionResolver` 引用 ng-core 的类型，不再循环。

---

## Crate 依赖图

```
ng-core (errors, version, utils, NameValidator, Token/Scope/Permission/Limit/TokenOrAuth)
  ↑
├── ng-db (entities + migrations + connection + registry + db RPC + nodeget-server::db methods)
│     ↑
│   ng-infra (DbBackedCache, ScopedPermission, PermissionResolver,
│             RpcDispatcher, make_global_cache!, rpc_exec! [server-only])
│     ↑
│   ┌──────┬────────┬─────────┬─────────┬──────────┐
│   ng-monitoring ng-token ng-kv  ng-task   ng-static
│     ↑           ↑                                 ↑
│     │         ng-terminal                     ng-crontab
│     │           ↑                              ↑  ↑
│     │           │                        ng-js-runtime
│     │           │                              ↑
│     │           │                        ng-js-worker
│     │           │                              ↑
│     └───────────┴──────────────────────────────┘
│                          ↑
│                nodeget-server (binary)
│                nodeget-agent  (binary → ng-agent-deps)
│
ng-config (独立，被 server/agent/ng-agent-deps 直接引用)
```

---

## 新 Crate 清单与文件映射

### Layer 0 — 基础设施

| Crate | 默认（types） | `server` feature 追加 | 来源 |
|-------|--------------|----------------------|------|
| **ng-core** | `NodegetError`, `JsonError`, `NodeGetVersion`, UUID 工具, NTP offset, server_json, error_message, `NameValidator` trait, **`Token`, `Limit`, `Scope`, `Permission`, `TokenOrAuth`** | — | `nodeget-lib/src/{error,utils}` + `nodeget-lib/src/permission/data_structure.rs` |
| **ng-db** | SeaORM entities (11表), `init_db_connection`, `DB` global (含 `get_db()`), `DbRegistry`, SQL 辅助, 行→JSON 转换, **`validate_db_name`** (impl `NameValidator`) | migration (17步), **`db` RPC 命名空间**, **`nodeget-server::database_storage` / `exec_sql` / `get_database_type`** | `server/src/{entity,db_connection,db_registry}` + `server/migration/` + `server/src/rpc/db/*` + `server/src/rpc/nodeget/{database_storage,exec_sql}.rs` |
| **ng-config** | `ServerConfig`, `AgentConfig`, `ServerArgs`, `AgentArgs`, CLI 解析, `SERVER_CONFIG` global, `RELOAD_NOTIFY` global, `palc` arg parsing | **`nodeget-server::read_config` / `edit_config` RPC** | `nodeget-lib/src/{config,args_parse}` + `server/src/rpc/nodeget/config.rs` |

> 所有 ng-* crate 位于 `crates/` 目录下。根目录仅保留 `server/`, `agent/`, `docs/`。

### Layer 1 — 基础设施抽象层

| Crate | 默认（types） | `server` feature 追加 | 来源 |
|-------|--------------|----------------------|------|
| **ng-infra** | `DbBackedCache` trait + `make_global_cache!` 宏, `ScopedPermission<T>` enum + `PermissionResolver` trait, `RpcDispatcher` trait | `rpc_exec!` 宏, `TruncatedRaw`, `RpcHelper` trait, `token_identity` | `server/src/cache/mod.rs` + `server/src/rpc/mod.rs` (通用部分) |

**关键**: ng-infra 默认不依赖 jsonrpsee。`rpc_exec!` 等仅在 `server` feature 下可用，避免 types 层拉入重依赖。

### Layer 2 — 业务 Crate

| Crate | 默认（types） | `server` feature 追加 | 额外注册 | 来源 |
|-------|--------------|----------------------|----------|------|
| **ng-monitoring** | `StaticMonitoringData`, `DynamicMonitoringData`, `DynamicMonitoringSummaryData`, query DSL, 过滤辅助 | RPC (agent/agent_uuid), `MonitoringBuffer`, 各缓存, **`nodeget-server::list_all_agent_uuid`** | `router()` — 无 | `nodeget-lib/src/monitoring` + `server/src/{monitoring_*,static_hash_cache,rpc/agent,rpc/agent_uuid}` + `server/src/rpc/nodeget/list_all_agent_uuid.rs` |
| **ng-token** | — (types 在 ng-core) | RPC, `TokenCache`, super-token, 生成/验证/轮换 | `router()` — 无 | `nodeget-lib/src/permission/{create,token_auth}` + `server/src/{token,rpc/token}` |
| **ng-kv** | `KVStore` types | RPC, namespace 管理, DB 读写, `ScopedPermission` 权限过滤 | `router()` — 无 | `nodeget-lib/src/kv` + `server/src/{kv,rpc/kv}` |
| **ng-task** | `TaskEventType`, `TaskEvent`, `TaskEventResult`, query DSL | `TaskManager`, RPC, 任务分发 | `router()` — 无 | `nodeget-lib/src/task` + `server/src/rpc/task` |
| **ng-crontab** | `Cron`, `CronType`, `CronResult` types, result query DSL | `CrontabCache`, scheduler, RPC (crontab + crontab_result) | `router()` — 无 | `nodeget-lib/src/{crontab,crontab_result}` + `server/src/{crontab,rpc/crontab,rpc/crontab_result}` |
| **ng-js-runtime** | `RuntimeLimits`, `RunType`, `CompileMode`, `JsCodeInput`, pool info types | QuickJS pool, 看门狗, bytecode 缓存, `server_runtime` init; `nodeget()` bridge 通过 `RpcDispatcher` 注入 | `router()` — 无 | `server/src/js_runtime/*` |
| **ng-js-worker** | `JsWorker` entity types, `JsResult` types, result query DSL | Worker CRUD, 执行服务, RPC, `ScopedPermission` 过滤 | **`router()` — `/worker-route/*`** | `nodeget-lib/src/{js_runtime,js_result}` + `server/src/rpc/{js_worker,js_result}` |
| **ng-static** | bucket/file 配置 types | `StaticCache`, CRUD, 文件上传/下载/WebDAV, RPC, impl `NameValidator` | **`router()` — `/nodeget/static/{name}` + WebDAV** | `server/src/{static_file,rpc/static_bucket,rpc/static_bucket_file}` |
| **ng-terminal** | auth types | WebSocket 代理, 会话管理, agent 检查 | **`router()` — terminal WS upgrade** | `server/src/terminal/*` |

### Layer 3 — Agent 依赖聚合

| Crate | 内容 | 来源 |
|-------|------|------|
| **ng-agent-deps** | 纯 re-export crate，聚合 agent 需要的所有类型 | 新建，re-export 来自 ng-core + ng-config + ng-monitoring + ng-task |

```rust
// crates/ng-agent-deps/src/lib.rs
pub use ng_core::*;
pub use ng_config::agent::{AgentConfig, AgentArgs, Server as AgentServer};
pub use ng_monitoring::data_structure::*;
pub use ng_task::{TaskEventType, TaskEvent, TaskEventResult, TaskEventResponse};
// Token/Scope/Permission/Limit/TokenOrAuth 已通过 ng_core::* re-export
```

### 删除

| Crate | 处理 |
|-------|------|
| **nodeget-lib** | Phase 4 完全移除 |

### 保留（精简）

| Crate | 变化 |
|-------|------|
| **nodeget-server** (binary) | 仅保留 `main.rs`, `logging/`, `rpc_timing.rs`, `subcommands/`, `rpc/nodeget/{hello,version,uuid,log_query,stream_log,self_update}.rs`; 通过 `init_all()` 初始化各 crate，merge 各 crate 的 RPC + router() |
| **nodeget-agent** (binary) | 依赖从 nodeget-lib → `ng-agent-deps`; `use nodeget_lib::` → `use ng_agent_deps::` |

---

## `nodeget-server` 命名空间方法分布

| 方法 | 注册所在 | 理由 |
|------|---------|------|
| `hello` | server binary | 无外部依赖 |
| `version` | server binary | 仅需 ng-core::NodeGetVersion |
| `uuid` | server binary | 仅需 ng-config::get_server_config |
| `list_all_agent_uuid` | ng-monitoring | 依赖 MonitoringUuidCache |
| `database_storage` | ng-db | 依赖 DB + pg_size / dbstat |
| `exec_sql` | ng-db | 依赖 DB 连接 |
| `get_database_type` | ng-db | 依赖 DB 连接 |
| `read_config` | ng-config | 依赖 SERVER_CONFIG + 配置文件 |
| `edit_config` | ng-config | 依赖配置文件 + RELOAD_NOTIFY |
| `log_query` | server binary | 依赖 logging 内存缓冲区 |
| `stream_log` | server binary | 依赖 logging 订阅通道 |
| `self_update` | server binary | 仅依赖 ng-core::version + ng-config |

---

## `server` Feature 设计

**所有含 server 端代码的 crate**（包括 ng-db, ng-config, ng-infra）统一使用此模式：

```toml
[features]
default = []
server = ["jsonrpsee", "sea-orm", "ng-db", "ng-infra/server", "tokio"]
```

- **默认**: 仅数据结构、类型定义、查询 DSL — agent / ng-agent-deps 可安全依赖
- **`server` feature**: 追加 RPC handler、DB 查询、缓存实现、缓冲区、`router()` — 仅 server binary 启用
- **ng-infra 的 `server` feature**: 启用 `rpc_exec!`、`TruncatedRaw`、`RpcHelper`、`token_identity`，引入 jsonrpsee optional 依赖
- 业务 crate 的 `server` feature 依赖 ng-infra 的 `server` feature：`ng-infra = { workspace = true, features = ["server"] }`

---

## Trait 设计详解

### ng-infra 默认 vs `server` feature 的精确边界

**默认可用（无重依赖）**：
- `ScopedPermission<T>` enum + `PermissionResolver` trait
- `RpcDispatcher` trait

**`server` feature 追加（需要 sea-orm + jsonrpsee）**：
- `AuthChecker` trait + 全局注入（NEW — 解决认证循环依赖）
- `DbBackedCache` trait + `make_global_cache!` 宏（需要 `DatabaseConnection`）
- `rpc_exec!` 宏 + `TruncatedRaw` + `RpcHelper` + `token_identity`（需要 jsonrpsee）

**为什么 `DbBackedCache` 移入 `server` feature**：此 trait 引用 `sea_orm::DatabaseConnection`，若放在默认层则 ng-agent-deps 传递拉入 sea-orm。Agent 不需要任何缓存框架。所有缓存实现都在 `server` feature 中，这是自然归属。

---

### `AuthChecker` — 认证函数循环依赖解决方案

**问题**: ng-db 的 `db` RPC 需要 `check_token_limit`（在 ng-token），ng-token 需要 ng-db entity。双向可选依赖仍被 Cargo 检测为循环。

**解决**: 定义 `AuthChecker` trait 在 ng-infra server feature，所有 RPC handler 通过全局注入调用：

```rust
/// ng-infra server feature
pub trait AuthChecker: Send + Sync {
    fn check_token_limit(&self, token: &TokenOrAuth, scopes: &[Scope], permissions: &[Permission]) -> Result<()>;
    fn check_super_token(&self, token: &str) -> Result<()>;
}

/// 全局注入，server binary 启动时调用
static AUTH_CHECKER: OnceLock<Box<dyn AuthChecker>> = OnceLock::new();
pub fn set_auth_checker(checker: Box<dyn AuthChecker>) { ... }
pub fn get_auth_checker() -> &'static dyn AuthChecker { ... }
```

ng-token 实现 `AuthChecker`（包装 `TokenCache::check_token_limit` + `check_super_token`）。server binary 在 `init_all()` 中注入。

**影响范围**: 所有 13 个 RPC 命名空间的 auth 模块，从 `ng_token::check_token_limit(...)` 改为 `ng_infra::get_auth_checker().check_token_limit(...)`。这是路径变更，不改逻辑。

---

### `ScopedPermission<T>` + `PermissionResolver`

```rust
/// ng-infra, 默认可用
pub enum ScopedPermission<T: Hash + Eq> {
    All,
    Scoped(HashSet<T>),
}

/// ng-infra, 默认可用；引用 ng-core 的 Token/Scope/Permission
pub trait PermissionResolver {
    type Resource: Hash + Eq;
    fn resolve(token: &Token, scope: &Scope, permission: &Permission) -> ScopedPermission<Self::Resource>;
}
```

---

### `RpcDispatcher` — JS bridge 回调注入

```rust
/// ng-infra, 默认可用
pub trait RpcDispatcher: Send + Sync {
    fn dispatch(&self, request: &str) -> String;
}
```

---

### `NameValidator` — 统一输入校验

```rust
/// ng-core, 默认可用
pub trait NameValidator: Sized {
    fn validate(name: &str) -> Result<Self>;
}
```

---

### `DbBackedCache` — 原有缓存框架

```rust
/// ng-infra server feature（需要 sea-orm）
pub trait DbBackedCache: Send + Sync {
    type Model;
    fn build_cache(models: Vec<Self::Model>) -> Self;
    fn reload_from_models(&mut self, models: Vec<Self::Model>);
    async fn load_all(db: &DatabaseConnection) -> Result<Vec<Self::Model>>;
}

/// server feature
macro_rules! make_global_cache { ... }
```

`make_global_cache!` 宏生成的代码引用 `DatabaseConnection` 类型，调用 crate 负责 import。宏定义本身不引入运行时依赖。

---

### JS Runtime API 注册拆分

**问题**: 当前 `js_runtime/mod.rs` 注册所有 JS API（`nodeget()`, `execSql()`, `db.*`, `inlineCall()`, fetch, buffer 等）。其中 `execSql()`/`db.*` 直接访问 DB，`inlineCall()` 调用 js_worker service——这些在 js_runtime 中不应有 ng-db/ng-js-worker 依赖。

**解决**: 拆分 API 注册为两层：

| 层 | 注册方 | JS API | 依赖 |
|---|--------|--------|------|
| **基础层** | ng-js-runtime | `nodeget()` (via RpcDispatcher), `randomUUID()`, fetch, buffer, stream | RpcDispatcher (注入) |
| **扩展层** | ng-js-worker | `execSql()`, `db.*`, `inlineCall()` | ng-db (直接访问), ng-infra |

ng-js-runtime 在创建 worker runtime 时暴露 hook：
```rust
/// ng-js-runtime server feature
pub trait JsApiRegistrar: Send + Sync {
    fn register(&self, context: &mut rquickjs::Context) -> Result<()>;
}
```

ng-js-worker 实现 `JsApiRegistrar`，server binary 注入。这样 js_runtime 不知道 DB/inlineCall 的存在。

---

### 不提取为 trait 的部分

| 原计划 trait | 改为 | 理由 |
|-------------|------|------|
| `CrateRegistrar` | server binary 中 `fn init_all(db)` | 只有 1 个实现者 |
| `BatchWriter<T>` | ng-monitoring 中直接实现 `MonitoringBuffer` | 只有 1 个实现者 |

---

## 脑内模拟发现的坑

### ng-core

| 坑 | 说明 | 处理 |
|---|------|------|
| `build.rs` 遗漏 | `NodeGetVersion` 通过 `vergen` build script 注入编译信息，当前在 nodeget-lib/build.rs | ng-core 需自带 build.rs + vergen dep |
| Token 方法拆分 | `Token` struct 可能有 `check_limit()` 等方法引用 TokenCache | struct + 纯数据方法 → ng-core；需 TokenCache 的方法 → ng-token |
| uuid v5 feature | `utils/uuid.rs` 的 `server_uuid_v5()` 需要 `uuid/v5` feature | workspace.dependencies 中 uuid 需加 `"v5"` feature |

### ng-db

| 坑 | 说明 | 处理 |
|---|------|------|
| generate_entity.sh 路径 | 脚本硬编码 `server/src/entity` 路径 | 更新脚本指向 `crates/ng-db/src/entity` |
| migration 引用 entity | migration 中 `use entity::xxx` 路径从 `crate::entity` 变为 ng-db 内部路径 | migration 保持 ng-db 子模块，`crate::entity` 路径不变 |
| db RPC auth | `rpc/db/auth.rs` 调用 `check_token_limit` → 需要 ng-token | 通过 `AuthChecker` trait (ng-infra) 注入，不直接依赖 ng-token |

### ng-infra

| 坑 | 说明 | 处理 |
|---|------|------|
| sea-orm feature unification | Cargo 对同版本 sea-orm 合并 features，ng-infra "minimal" 别名无效 | 删掉 `sea-orm-minimal` 别名，`DbBackedCache` 移入 `server` feature，ng-infra 默认零 DB 依赖 |

### ng-config

| 坑 | 说明 | 处理 |
|---|------|------|
| AgentConfig 异步加载 | `AgentConfig::get_and_parse_config()` 用 `tokio::fs`，agent 不启用 server feature | ng-config 默认需 `tokio/fs` feature（轻量，agent 也需要） |
| palc 依赖 | CLI arg 解析用 `palc` crate | 加入 ng-config 默认依赖 |

### ng-monitoring

| 坑 | 说明 | 处理 |
|---|------|------|
| 模块体积最大 | ~15 个源文件，含 3 种缓存 + buffer + 多个 RPC | 需要最多工时；内部分层清晰（types / cache / buffer / rpc） |
| MonitoringBuffer flush 顺序 | flush 线程和 graceful shutdown drain 需要正确的 tokio spawn 管理 | 纯搬迁，不重构 flush 逻辑 |

### ng-token

| 坑 | 说明 | 处理 |
|---|------|------|
| Token struct 方法拆分 | `Token` 可能有 impl 块引用 TokenCache | 仅 data struct + serde → ng-core；impl 方法按需拆到 ng-token |
| subtle crate | `check_super_token` 用 `subtle::ConstantTimeEq` 做常量时间比较 | ng-token server feature 需要 subtle dep |

### ng-js-runtime

| 坑 | 说明 | 处理 |
|---|------|------|
| server_runtime init | `init(tokio::Handle)` 存储到全局，block_in_place 用 | 纯搬迁 |
| ARM bindgen | rquickjs ARM target 需要 `bindgen` feature | ng-js-runtime Cargo.toml 单独 target override |
| llrt_* 6 个依赖 | llrt_fetch/buffer/timers/url/stream_web/util | 全部仅在 `server` feature |

### ng-js-worker

| 坑 | 说明 | 处理 |
|---|------|------|
| inline_call 反向依赖 | js_runtime 的 `inlineCall()` 调 js_worker service | 通过 `JsApiRegistrar` 反转：js-worker 注册 inlineCall API |
| route_name.rs | `/worker-route/*` HTTP handler，用 `enter_runtime()` 进 tokio 上下文 | 通过 `router()` 暴露给 server binary |

### ng-static

| 坑 | 说明 | 处理 |
|---|------|------|
| dav-server 传递 | WebDAV handler 依赖重，agent 不应拉入 | `server` feature gate |

### ng-terminal

| 坑 | 说明 | 处理 |
|---|------|------|
| TaskManager 依赖 | check_agent 可能用 TaskManager 查 agent 在线状态 | 需确认；若依赖则 ng-terminal server 需 ng-task dep |
| axum WS | WebSocket upgrade 需 axum dep | `server` feature gate |

### server binary

| 坑 | 说明 | 处理 |
|---|------|------|
| 注入点过多 | AuthChecker + RpcDispatcher + JsApiRegistrar + DbAccessor(如需) | `init_all()` 统一注入所有 trait 实现 |
| self_update | 下载+替换+重启，需 reqwest dep | server binary 直接依赖 reqwest |

### agent binary

| 坑 | 说明 | 处理 |
|---|------|------|
| `use nodeget_lib::` 批量替换 | agent 所有源文件中的 import 路径 | 全局 find-replace，验证编译 |

---

## 阶段计划

### Phase 0 — 分支创建 + 骨架搭建 [XS, ~1h]

**目标**: 创建 `dev-ref` 分支，14 个空 crate 加入 workspace，编译通过。

- 从 `dev` 创建 `dev-ref` 分支
- 创建 `crates/` 目录，内建 14 个 crate 子目录及最小 `Cargo.toml` + `lib.rs`
- 更新根 `Cargo.toml` workspace members
- `cargo check --workspace` 通过（空 crate）

**验证**: `cargo check --workspace` 成功，原有 server/agent 编译不受影响。

**回滚**: 删除 `crates/` 目录，恢复根 Cargo.toml，切回 `dev` 分支。

---

### Phase 0.5 — jsonrpsee merge POC [S, ~2h]

**目标**: 验证多 crate `RpcModule<()>` merge + 同名命名空间分散注册 + axum Router `.merge()` 组合。

- 在 2 个 crate 中各创建 1 个 mock RPC method（不同命名空间）
- 在 2 个 crate 中各注册同名命名空间 `nodeget-server` 的 1 个 method，验证 merge 无冲突
- 在 2 个 crate 中各暴露 `fn router() -> axum::Router`，server binary `.merge()` 组合
- 启动 HTTP server，发送 JSON-RPC + HTTP 请求验证全链路

**验证**: 所有 method 可调用，同名命名空间合并正常，axum 路由组合正常。

**失败后果**: 退回集中注册模式——所有 RPC/路由在 server binary 中统一注册，业务 crate 只暴露纯函数。

**回滚**: 删除 mock 代码，crate 回归空骨架。

---

### Phase 1 — 基础设施层 [M, 1-2 天]

**目标**: ng-core, ng-db, ng-infra, ng-config 可独立编译。

**任务**:

| # | 任务 | 受影响文件 | 依赖 |
|---|------|-----------|------|
| 1.1 | **ng-core**: `NodegetError`, `JsonError`, `NodeGetVersion`, UUID 工具, NTP offset, server_json, error_message, `NameValidator` trait, **`Token`, `Limit`, `Scope`, `Permission`, `TokenOrAuth`** (从 permission/data_structure 搬入) | `nodeget-lib/src/{error.rs,utils/*}` + `nodeget-lib/src/permission/data_structure.rs` → `crates/ng-core/src/*` | 无 |
| 1.2 | **ng-db** (默认): entity, `init_db_connection`, `DB` global, `DbRegistry`, SQL 辅助, `validate_db_name`; **ng-db** (`server`): migration, `db` RPC, `nodeget-server::database_storage/exec_sql/get_database_type` | `server/src/{entity,db_connection,db_registry}` + `server/migration/` + `server/src/rpc/db/*` + `server/src/rpc/nodeget/{database_storage,exec_sql}.rs` → `crates/ng-db/` | ng-core |
| 1.3 | **ng-infra** (默认): `DbBackedCache`, `make_global_cache!`, `ScopedPermission`, `PermissionResolver`, `RpcDispatcher`; **ng-infra** (`server`): `rpc_exec!`, `TruncatedRaw`, `RpcHelper`, `token_identity` | `server/src/cache/mod.rs` + `server/src/rpc/mod.rs` → `crates/ng-infra/src/*` | ng-core (PermissionResolver 引用 Token/Scope/Permission) |
| 1.4 | **ng-config** (默认): `ServerConfig`, `AgentConfig`, `ServerArgs`, `AgentArgs`, CLI 解析, `SERVER_CONFIG`, `RELOAD_NOTIFY`; **ng-config** (`server`): `read_config` / `edit_config` RPC | `nodeget-lib/src/{config,args_parse}` + `server/src/rpc/nodeget/config.rs` → `crates/ng-config/` | ng-core |

**执行策略**: 纯搬迁，仅调 `use`/`pub`。新增的 trait/类型定义不改现有行为。**权限类型从 ng-token 前置搬入 ng-core**（task 1.1），确保 ng-infra 的 `PermissionResolver` 不产生循环。

**验证**:
- 每个 crate `cargo check --package <name>` 通过
- `cargo check --package <name> --features server` 通过（ng-db, ng-config, ng-infra）
- 原 `cargo check --package nodeget-server` 仍通过
- 原 `cargo check --package nodeget-agent` 仍通过

**回滚**: 移除新 crate 依赖，恢复 server/lib 原有代码路径。

---

### Phase 2 — 独立业务 Crate [L, 3-5 天, 高度可并行]

**目标**: 5 个无跨业务依赖的 crate 提取完成。

**任务**:

| # | 任务 | 依赖 | 可并行 |
|---|------|------|--------|
| 2.1 | **ng-token**: TokenCache + super-token + RPC (types 已在 ng-core) | Phase 1 | ✅ |
| 2.2 | **ng-kv**: KV types + namespace 管理 + RPC + `ScopedPermission` 过滤 | Phase 1 | ✅ |
| 2.3 | **ng-task**: task types + TaskManager + RPC | Phase 1 | ✅ |
| 2.4 | **ng-monitoring**: 全部监控数据 + 缓存 + 缓冲区 + RPC + `nodeget-server::list_all_agent_uuid` | Phase 1 | ✅ |
| 2.5 | **ng-static**: bucket/file 管理 + 缓存 + RPC + WebDAV + `router()` + impl `NameValidator` | Phase 1 | ✅ |

**约束**: 纯迁移铁律 + `MonitoringBuffer` 直接 struct 不走 trait + `router()` 仅在 `server` feature 下。

**验证**: 每个 crate `cargo check --package <name> --features server` + server binary merge 通过。

**并行策略**: 5 个 agent 并行。

---

### Phase 3 — 有依赖的业务 Crate [M, 2-3 天]

| # | 任务 | 依赖 | 可并行 |
|---|------|------|--------|
| 3.1 | **ng-js-runtime**: QuickJS pool + 看门狗 + bytecode 缓存 + `RpcDispatcher` 注入; ARM target 需额外 `bindgen` feature | Phase 1 | ✅ |
| 3.2 | **ng-js-worker**: worker 管理 + 执行 + RPC + `ScopedPermission` + **`router()` (`/worker-route/*`)** | 3.1 | ❌ |
| 3.3 | **ng-crontab**: scheduler + cache + RPC | ng-task(2.3), ng-js-runtime(3.1) | ❌ |
| 3.4 | **ng-terminal**: WebSocket 代理 + auth + **`router()` (terminal WS)** | ng-token(2.1) | ✅ |

**ng-js-runtime ARM 处理**: ng-js-runtime 的 `Cargo.toml` 需单独 target override：
```toml
[target.'cfg(target_arch = "arm")'.dependencies]
rquickjs = { git = "https://github.com/delskayn/rquickjs.git", default-features = false, features = ["futures", "bindgen"] }
```

**验证**: 同 Phase 2。

---

### Phase 4 — 集成 + agent-deps + nodeget-lib 移除 [M, 2-3 天]

| # | 任务 | 依赖 |
|---|------|------|
| 4.1 | 创建 ng-agent-deps: re-export crate | Phase 2 (types 就绪) |
| 4.2 | Server binary 瘦身: `init_all(db)` + merge RPC + merge `router()` | Phase 2+3 |
| 4.3 | Server binary 注册 `nodeget-server` 自身方法 + logging 路由 | 4.2 |
| 4.4 | Agent binary: nodeget-lib → ng-agent-deps; `use nodeget_lib::` → `use ng_agent_deps::` | 4.1 |
| 4.5 | 删除 nodeget-lib | 4.4 完成 |
| 4.6 | Server binary 移除已迁移代码 | 4.2 + 4.3 |
| 4.7 | 更新 workspace Cargo.toml | 4.5 + 4.6 |

**验证**: `cargo check --workspace` + `cargo clippy --workspace` + server 启动 + agent 连接。

**回滚**: 保留 nodeget-lib 直到 4.5 之前。

---

### Phase 5 — 清理与验证 [S, 0.5-1 天]

| # | 任务 | 依赖 |
|---|------|------|
| 5.1 | 清理 server/src/ 残留 | 4.7 |
| 5.2 | 更新 CLAUDE.md | 4.7 |
| 5.3 | 更新 Dockerfile | 4.7 |
| 5.4 | 全量 clippy | 5.1-5.3 |

**验证**: `cargo clippy --workspace` 0 errors。

---

## Pre-Mortem

| # | 风险 | 概率 | 影响 | 缓解 |
|---|------|------|------|------|
| 1 | **循环依赖**: ng-infra ↔ ng-token | ~~高~~ 已解决 | ~~阻塞~~ | 权限类型前置到 ng-core，ng-token 仅做 server 端实现 |
| 2 | **JS bridge 循环**: ng-js-runtime `nodeget()` | 高 | 阻塞 | `RpcDispatcher` trait 在 ng-infra，ng-js-runtime 不依赖业务 crate |
| 3 | **jsonrpsee merge 不兼容** | 高 | 架构推翻 | Phase 0.5 POC；失败退回集中注册 |
| 4 | **ng-infra 默认拉入 jsonrpsee**: `rpc_exec!` 等需要 jsonrpsee 类型 | ~~中~~ 已解决 | ~~阻塞~~ | `rpc_exec!`/`TruncatedRaw`/`RpcHelper`/`token_identity` 全部 gate 在 `server` feature |
| 5 | **SeaORM entity 路径变更** | 中 | 编译失败 | migration 保持 ng-db 内部子模块 |
| 6 | **OnceLock init 顺序** | 中 | 运行时 panic | `init_all()` 按拓扑顺序显式调用 |
| 7 | **`pub(crate)` → `pub` 暴露过度** | 中 | API 污染 | 最小化 pub 集合，`#[doc(hidden)]` 标记不稳定类型 |
| 8 | **构建时间增加** | 低 | 开发体验 | workspace 共享 build profile |
| 9 | **Agent 编译断裂** | 中 | 阻塞 | ng-agent-deps re-export 同路径；4.4 先更新 agent |
| 10 | **WebDAV 依赖传递** | 低 | 依赖膨胀 | `server` feature gate dav-server |
| 11 | **缓存 reload 协调** | 低 | 数据不一致 | serve.rs 统一 reload 顺序 |
| 12 | **Release LTO** | 低 | 性能 | 重构后建议开启 LTO |
| 13 | **搬迁偷改逻辑** | 高 | 引入回归 | diff 审查：函数体 1:1 一致 |
| 14 | **`nodeget-server` 命名空间分散冲突** | 低 | merge 失败 | 每个 method 只在一个 crate 注册 |
| 15 | **ng-infra 默认需 sea-orm**: `DbBackedCache` 引用 `DatabaseConnection` | 中 | 依赖膨胀 | ng-infra 仅启用 sea-orm minimal features（无 sqlx-* 驱动）；驱动 feature 留给 ng-db `server` |

---

## 预估时间线

| Phase | 工期 | 可并行 |
|-------|------|--------|
| Phase 0 | 1h | - |
| Phase 0.5 | 2h | - |
| Phase 1 | 1-2 天 | 1.1-1.4 可部分并行 |
| Phase 2 | 3-5 天 | **5 个 crate 完全并行** |
| Phase 3 | 2-3 天 | 3.1 ‖ 3.4, 然后 3.2 ‖ 3.3 |
| Phase 4 | 2-3 天 | 4.1+4.4 可与 4.2+4.3 并行 |
| Phase 5 | 0.5-1 天 | - |
| **总计** | **9-15 天** | 最大并行度 5 |

---

## 新 Workspace 目录结构

```
NodeGet/
├── Cargo.toml              # workspace root
├── crates/
│   ├── ng-core/            # errors, version, utils, NameValidator, Token/Scope/Permission/Limit/TokenOrAuth
│   ├── ng-db/              # entities, migrations, connection, registry, db RPC
│   │   └── migration/      # SeaORM migrations (子模块, 仅 server feature)
│   ├── ng-infra/           # DbBackedCache + ScopedPermission + PermissionResolver +
│   │                       # RpcDispatcher + make_global_cache! + rpc_exec!(server-only)
│   ├── ng-config/          # config + CLI args + config RPC + SERVER_CONFIG/RELOAD_NOTIFY
│   ├── ng-monitoring/      # 监控数据 + RPC + 缓存 + 缓冲区 + list_all_agent_uuid
│   ├── ng-token/           # TokenCache + super-token + RPC (types 在 ng-core)
│   ├── ng-kv/              # KV 存储 + RPC + ScopedPermission
│   ├── ng-task/            # 任务类型 + TaskManager + RPC
│   ├── ng-crontab/         # 定时任务 + scheduler + RPC
│   ├── ng-js-runtime/      # QuickJS 运行时池 + RpcDispatcher bridge
│   ├── ng-js-worker/       # JS Worker + RPC + router(/worker-route/*)
│   ├── ng-static/          # 静态文件桶 + WebDAV + RPC + router(/nodeget/static/*)
│   ├── ng-terminal/        # WebSocket 终端 + router(WS upgrade)
│   └── ng-agent-deps/      # Agent 依赖聚合 (纯 re-export)
├── server/                 # Server binary (薄入口)
│   └── src/
│       ├── main.rs
│       ├── logging/
│       ├── rpc_timing.rs
│       ├── rpc/nodeget/    # hello, version, uuid, log_query, stream_log, self_update
│       └── subcommands/
├── agent/                  # Agent binary → ng-agent-deps
└── docs/
```

根 `Cargo.toml` workspace members：
```toml
[workspace]
members = [
    "server",
    "agent",
    "crates/ng-core",
    "crates/ng-db",
    "crates/ng-db/migration",
    "crates/ng-infra",
    "crates/ng-config",
    "crates/ng-monitoring",
    "crates/ng-token",
    "crates/ng-kv",
    "crates/ng-task",
    "crates/ng-crontab",
    "crates/ng-js-runtime",
    "crates/ng-js-worker",
    "crates/ng-static",
    "crates/ng-terminal",
    "crates/ng-agent-deps",
]
resolver = "2"

[workspace.package]
version = "0.5.0"
license = "AGPL-3"
edition = "2024"
repository = "https://github.com/NodeSeekDev/NodeGet"

# ── 统一依赖版本 ────────────────────────────────────────────────
# 原则：default-features = false，仅显式启用所需 features
[workspace.dependencies]
# 基础
serde = { version = "1.0", default-features = false, features = ["std", "derive"] }
serde_json = { version = "1.0", default-features = false, features = ["std"] }
anyhow = { version = "1.0", default-features = false }
tokio = { version = "1.48.0", default-features = false, features = ["macros", "rt", "rt-multi-thread", "sync", "time"] }
uuid = { version = "1.19.0", default-features = false, features = ["std", "serde"] }
chrono = { version = "0.4.43", default-features = false, features = ["now", "std"] }
toml = { version = "1.1.2", default-features = false, features = ["std", "serde", "parse"] }
sha2 = { version = "0.11.0", default-features = false }
thiserror = { version = "2.0.18", default-features = false }
rand = { version = "0.9", default-features = false, features = ["std", "thread_rng"] }
base64 = { version = "0.22.1", default-features = false, features = ["alloc"] }
hex = { version = "0.4.3", default-features = false, features = ["std"] }
url = { version = "2.5.8", default-features = false, features = ["std", "serde"] }
futures-util = { version = "0.3.31", default-features = false }
subtle = { version = "2.6.1", default-features = false }
palc = { version = "0.0.2", default-features = false, features = ["help"] }

# DB
sea-orm = { version = "2.0.0-rc.38", default-features = false, features = ["runtime-tokio-rustls", "sqlx-sqlite", "sqlx-postgres", "macros", "with-json", "with-uuid"] }
# ng-infra 仅需 minimal sea-orm (无驱动)，各 crate 可按需启用更多 feature
sea-orm-minimal = { version = "2.0.0-rc.38", default-features = false, features = ["runtime-tokio-rustls", "macros", "with-json", "with-uuid"] }

# RPC / HTTP
jsonrpsee = { git = "https://github.com/infinitefield/jsonrpsee.git", default-features = false, features = ["server", "macros"] }
axum = { version = "0.8.8", default-features = false, features = ["tokio", "ws", "http1", "query"] }
reqwest = { version = "0.13", default-features = false, features = ["rustls", "json"] }

# JS Runtime
rquickjs = { git = "https://github.com/delskayn/rquickjs.git", default-features = false, features = ["futures"] }
llrt_fetch = { git = "https://github.com/awslabs/llrt.git", default-features = false, features = ["http1", "compression-rust", "webpki-roots", "tls-ring"] }
llrt_buffer = { git = "https://github.com/awslabs/llrt.git", default-features = false }
llrt_timers = { git = "https://github.com/awslabs/llrt.git", default-features = false }
llrt_url = { git = "https://github.com/awslabs/llrt.git", default-features = false }
llrt_stream_web = { git = "https://github.com/awslabs/llrt.git", default-features = false }
llrt_util = { git = "https://github.com/awslabs/llrt.git", default-features = false }

# Logging
tracing = { version = "0.1", default-features = false, features = ["std"] }
tracing-subscriber = { version = "0.3", default-features = false, features = ["env-filter", "json", "fmt", "chrono", "tracing-log"] }
log = { version = "0.4.29", default-features = false, features = ["std"] }

# WebDAV
dav-server = { version = "0.11.0", default-features = false, features = ["localfs"] }

# Cron
cron = { version = "0.16.0", default-features = false }

# Internal crates
ng-core = { path = "crates/ng-core" }
ng-db = { path = "crates/ng-db" }
ng-infra = { path = "crates/ng-infra" }
ng-config = { path = "crates/ng-config" }
ng-monitoring = { path = "crates/ng-monitoring" }
ng-token = { path = "crates/ng-token" }
ng-kv = { path = "crates/ng-kv" }
ng-task = { path = "crates/ng-task" }
ng-crontab = { path = "crates/ng-crontab" }
ng-js-runtime = { path = "crates/ng-js-runtime" }
ng-js-worker = { path = "crates/ng-js-worker" }
ng-static = { path = "crates/ng-static" }
ng-terminal = { path = "crates/ng-terminal" }
ng-agent-deps = { path = "crates/ng-agent-deps" }
migration = { path = "crates/ng-db/migration" }
```

各子 crate `Cargo.toml` 示例：

```toml
# crates/ng-infra/Cargo.toml — 展示 server feature gate
[package]
name = "ng-infra"
version.workspace = true
license.workspace = true
edition.workspace = true
repository.workspace = true

[dependencies]
ng-core = { workspace = true }
sea-orm = { workspace = true, features = ["runtime-tokio-rustls", "macros", "with-json", "with-uuid"] }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }

[features]
default = []
server = ["jsonrpsee", "tokio"]

[dependencies.jsonrpsee]
workspace = true
optional = true

[dependencies.tokio]
workspace = true
optional = true
```

```toml
# crates/ng-agent-deps/Cargo.toml
[package]
name = "ng-agent-deps"
version.workspace = true
license.workspace = true
edition.workspace = true
repository.workspace = true

[dependencies]
ng-core = { workspace = true }
ng-config = { workspace = true }
ng-monitoring = { workspace = true }   # 默认 feature (types only)
ng-task = { workspace = true }          # 默认 feature (types only)
# ng-token types (Token/Scope/Permission) 已通过 ng-core 传递
```
