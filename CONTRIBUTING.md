# 贡献到本项目

你可以通过以下方式为 NodeGet 后端项目作出贡献

## Issue

提出 Issue 以**请求新功能**或**报告 Bugs**

如果为报告 Bugs，至少需要提供以下信息:

- Server / Agent 的错误
- 运行的平台架构
- 系统版本（发行版）和小版本号
- 详细的过程说明
- 请求体（如果有）
- 日志等级至少为 `DEBUG` 的日志文件
- 高级用户可以提供自己运行的对照试验测试，以帮助开发者更快地定位问题

请自行确保所有信息安全，不包含敏感信息

如果为新功能请求，请提供以下信息:

- 详细的功能描述
- 是否可以通过 JS Worker 功能部分 / 完全实现
- 社区中是否有广泛需求，如有请贴上链接
- 是否为特定平台特化实现，如为部分设备添加温度检测等
- 你，或者是否已经有人在实现该功能，即将 PR 到主仓库

新功能的实现优先度会在后期，根据社区需求和实现难度进行评估后再决定开发速度

## Pull Request

请务必在提交 Pull Request 前先提出 Issue 以讨论实现细节，并且在 PR 中注明关联的 Issue 编号

**本项目禁止任何纯 AI Agent 生成的且不经具有 Rust 经验的开发者审查的 PR**

请你按照以下优先级审查 PR 的代码:

- 代码是否可以通过现有的 CI Build Workflow 进行跨平台构建（特别是涉及到 Agent 的更改，必须保证现有的平台支持不变）
- 代码是否引入了新的依赖库？如有，新的依赖库是否被 Rust 社区广泛使用，是否不明显影响项目的构建时间和体积，是否已经最小化
  Features，是否有同类小型库可替
- 是否同步更新文档说明
- 禁止任何地方代码在正常运行时，以任何形式退出程序程序执行环境
- 代码格式化，提交前在项目根目录运行 `cargo fmt`，以确保代码风格一致（不要使用 RustRover 等 IDE 工具自带的代码格式化工具）

所有不具有在主项目中 Push 到 dev 分支的开发者，都应先行 Fork 项目后，在自己的分支中的 dev 进行开发，完成后提交 PR 到主项目的
dev 分支。

---

## Workspace 模块说明

### Crate 依赖图

```
ng-core (error, version, utils, Token/Scope/Permission/Limit/TokenOrAuth)
  ↑
├── ng-db (entities, DB 连接, DbRegistry, db RPC)
│     ↑
│   ng-infra (DbBackedCache, rpc_exec!, AuthChecker, RpcHelper [server-only])
│     ↑
│   ┌──────┬────────┬─────────┬─────────┐
│   ng-monitoring ng-token ng-kv  ng-task
│     ↑           ↑                ↑
│     │         ng-terminal   ng-crontab
│     │           ↑              ↑  ↑
│     │           │        ng-js-runtime
│     │           │              ↑
│     │           │        ng-js-worker
│     │           │              ↑
│     └───────────┴──────────────┘
│                        ↑
│   ng-static (独立，仅依赖 ng-infra/ng-db/ng-config)
│
ng-config (独立，被 server/agent 直接引用)
```

### 各 Crate 职责

| Crate              | 职责                                                                                     | Feature                             | 关键导出                                                                                                      |
|--------------------|----------------------------------------------------------------------------------------|-------------------------------------|-----------------------------------------------------------------------------------------------------------|
| **ng-core**        | 基础类型：错误枚举、权限数据结构、版本信息、工具函数                                                             | `for-server`/`for-agent`（启用 `libc`） | `NodegetError`, `Token`, `Scope`, `Permission`, `Limit`, `TokenOrAuth`, `NameValidator`, `NodeGetVersion` |
| **ng-db**          | 数据库层：SeaORM 实体、连接初始化、全局单例、DB 注册表、`db` RPC                                              | `server`                            | `get_db()`, `init_db_connection()`, `DbRegistryManager`, `row_to_json()`, `validate_db_name()`            |
| **ng-infra**       | 共享基础设施：缓存框架、RPC 宏、认证 trait（`server` feature）                                           | `server`                            | `DbBackedCache`, `make_global_cache!`, `rpc_exec!`, `AuthChecker`, `RpcHelper`, `TruncatedRaw`            |
| **ng-config**      | 配置解析与管理：Server/Agent 配置、CLI 参数、全局配置单例、`read/edit_config` RPC                           | `server`                            | `ServerConfig`, `AgentConfig`, `get_server_config()`, `get_reload_notify()`                               |
| **ng-monitoring**  | 监控数据：数据结构、3 种缓存、批量写入缓冲区、`agent`/`agent-uuid`/`nodeget-server::list_all_agent_uuid` RPC | `server`                            | `StaticMonitoringData`, `DynamicMonitoringData`, `MonitoringBuffer`, `rpc_module()`                       |
| **ng-token**       | Token 管理：缓存、生成、super-token、认证函数、`token` RPC                                            | `server`                            | `TokenCache`, `check_token_limit()`, `check_super_token()`, `register_auth_checker()`                     |
| **ng-kv**          | KV 存储：命名空间管理、JSON 值读写、`kv` RPC                                                         | `server`                            | `KVStore`, `TokenPermissionChecker`, `NAMESPACE_MARKER_KEY`                                               |
| **ng-task**        | 任务系统：类型定义、TaskManager、广播分发、`task` RPC                                                  | `server`                            | `TaskEventType`, `TaskManager`, `TaskAuthProvider`, `MonitoringUuidProvider`                              |
| **ng-crontab**     | 定时任务：类型、调度器、缓存、`crontab`/`crontab_result` RPC                                          | `server`                            | `CronType`, `CrontabCache`, `init_crontab_worker()`, `JsWorkerScheduler`                                  |
| **ng-js-runtime**  | JS 运行时：QuickJS 线程池、字节码编译、API 注入                                                        | `server`                            | `spawn_on_server_runtime()`, `JsWorkerService`, `RawJsonDispatcher`, `RuntimeLimits`                      |
| **ng-js-worker**   | JS Worker 记录管理：CRUD、执行调度、`js-worker`/`js-result` RPC                                   | `server`                            | `enqueue_defined_js_worker_run()`, `run_inline_call_and_record_result()`, `rpc_module()`                  |
| **ng-static**      | 静态文件服务：Bucket 管理、上传/下载、WebDAV、`static-bucket`/`static-bucket-file` RPC                 | `server`                            | `StaticCache`, `router()`, `validate_name()`, `validate_sub_path()`                                       |
| **ng-terminal**    | WebSocket 终端：会话管理、Agent 检查、`terminal` WS handler                                       | `server`                            | `router()`, `TerminalState`, `TokenPermissionChecker`                                                     |
| **ng-migration**   | SeaORM 数据库迁移（19 步）                                                                     | 无                                   | `Migrator`                                                                                                |
| **nodeget-server** | 服务端二进制：薄入口，组装所有 RPC + HTTP 路由 + 初始化注入                                                  | 无                                   | —                                                                                                         |
| **nodeget-agent**  | Agent 二进制：监控采集、多服务器连接、任务执行                                                             | 无                                   | —                                                                                                         |

---

## 编码规范

### 错误处理

项目使用三层错误体系：

| 层级     | 类型                                            | 使用场景                                                  |
|--------|-----------------------------------------------|-------------------------------------------------------|
| 领域错误   | `NodegetError` 枚举（11 变体，带数字 code 101-108/999） | 构造可克隆的、有 code 的业务错误，始终通过 `.into()` 转为 `anyhow::Error` |
| 内部传递   | `anyhow::Result<T>`（ng-core 导出为 `Result<T>`）  | 所有内部函数的返回类型                                           |
| RPC 边界 | `RpcResult<Box<RawValue>>`                    | 仅用于 RPC handler 函数签名                                  |

**RPC handler 标准错误桥接模式**：

```rust
pub async fn some_method(...) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 1. 权限检查
        check_permission(&token, ...).await?;
        // 2. 业务逻辑
        let data = do_something().await?;
        // 3. 序列化
        RawValue::from_string(serde_json::to_string(&data)?)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };
    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
```

### RPC 方法四层结构

每个 RPC 方法必须拆分为四层：

**1. Trait 定义**（`rpc/mod.rs`，使用 `#[rpc]` proc macro）：

```rust
#[rpc(server, namespace = "kv")]
pub trait Rpc {
    #[method(name = "get_value")]
    async fn get_value(&self, token: String, namespace: String, key: String)
                       -> RpcResult<Box<RawValue>>;
}
```

**2. Impl + tracing span**（`rpc/mod.rs`，使用 `token_identity` + `info_span!` + `rpc_exec!`）：

```rust
#[async_trait]
impl RpcServer for KvRpcImpl {
    async fn get_value(&self, token: String, ...) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "kv", "kv::get_value",
            token_key = tk, username = un, namespace = %namespace, key = %key);
        async { rpc_exec!(get_value::get_value(token, namespace, key).await) }
            .instrument(span)
            .await
    }
}
```

**3. Handler 函数**（`rpc/<method_name>.rs`，独立文件）：

```rust
pub async fn get_value(token: String, namespace: String, key: String)
                       -> RpcResult<Box<RawValue>>
{
    // 标准桥接模式（见上方错误处理章节）
}
```

**4. 模块注册**（`rpc/mod.rs` 底部）：

```rust
pub fn rpc_module() -> jsonrpsee::RpcModule<KvRpcImpl> {
    KvRpcImpl.into_rpc()
}
```

### 权限检查（Auth）模式

每个含 RPC 的业务 crate 定义独立的 `auth.rs`，使用 `TokenPermissionChecker` trait + `OnceLock` 注入：

```rust
// auth.rs 标准模板
pub trait TokenPermissionChecker: Send + Sync {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> Pin<Box<dyn Future<Output=anyhow::Result<bool>> + Send + '_>>;

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> Pin<Box<dyn Future<Output=anyhow::Result<bool>> + Send + '_>>;
}

static TOKEN_CHECKER: OnceLock<Box<dyn TokenPermissionChecker>> = OnceLock::new();

pub fn set_token_checker(checker: Box<dyn TokenPermissionChecker>) {
    let _ = TOKEN_CHECKER.set(checker);
}
```

Auth 函数标准流程：`TokenOrAuth::from_full_token` → `get_token_checker().check_token_limit()` → 判断 → `Ok(())` 或
`NodegetError::PermissionDenied`。

Server binary 在 `serve.rs` 中注入所有 trait 实现，所有实现最终委托给 `ng_token` 函数。

### OnceLock 全局注入模式

所有跨 crate 依赖通过 `OnceLock` + `set_*()/get_*()` 注入。约定：

- `set_*` 静默忽略重复初始化：`let _ = LOCK.set(val);`
- `get_*` 未初始化时 panic：`.expect("... not initialized -- call set_* first")`
- 部分函数返回 `Option`（如 `ng_db::get_db()`）而非 panic

现有注入点（server binary `serve.rs` 统一注册）：

| 注入函数                           | 定义 Crate                                    | 用途                        |
|--------------------------------|---------------------------------------------|---------------------------|
| `set_auth_checker`             | ng-infra                                    | 认证 → Token 元数据            |
| `set_auth_provider`            | ng-db                                       | db 命名空间权限                 |
| `set_token_checker`            | ng-kv, ng-static, ng-terminal, ng-js-worker | 各自命名空间权限                  |
| `set_auth_provider`            | ng-task                                     | task 命名空间权限               |
| `set_monitoring_uuid_provider` | ng-task                                     | UUID 缓存访问                 |
| `set_check_super_token_fn`     | ng-config                                   | config RPC super-token 校验 |
| `set_js_worker_service`        | ng-js-runtime                               | JS 执行调度                   |
| `set_js_worker_scheduler`      | ng-crontab                                  | cron → JS worker 调度       |

### Feature Gate 模式

所有业务 crate 使用统一 feature 模式：

```toml
[features]
default = []
server = ["dep:jsonrpsee", "dep:sea-orm", ...]  # 所有重依赖
```

- **default**：仅类型、数据结构、查询 DSL — agent 可安全依赖
- **server**：RPC handler、DB 查询、缓存、缓冲区 — 仅 server binary 启用
- 例外：`ng-core` 使用 `for-server`/`for-agent`（均仅启用 `libc`）

Agent 依赖：`ng-core/for-agent` + `ng-config` + `ng-task` + `ng-monitoring`（均无 `server` feature）。

### 缓存模式

所有"全量加载"缓存使用 `ng_infra::server::DbBackedCache` trait + `make_global_cache!` 宏：

```rust
// 定义 trait impl
impl DbBackedCache for TokenCache {
    type Model = token::Model;
    fn cache_name() -> &'static str { "token" }
    fn build_cache(models: Vec<Self::Model>) -> Self { ... }
    async fn reload_from_models(&self, models: Vec<Self::Model>) { ... }  // 注意 &self，非 &mut self
    async fn load_all() -> anyhow::Result<Vec<Self::Model>> { load_from_db::<token::Entity>().await }
}

// 生成全局单例
make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);
// → init() / global() / reload() 方法自动生成
```

现有 DB-backed 缓存（使用宏）：TokenCache, CrontabCache, StaticCache, MonitoringUuidCache。
纯内存缓存（手写 `OnceLock` 单例，非 DB 全量加载）：MonitoringLastCache, StaticHashCache。

### Serde 约定

- 所有枚举和结构体：`#[serde(rename_all = "snake_case")]`
- 小写枚举变体：`#[serde(rename_all = "lowercase")]`（如 `IpProvider`）
- JSON 列：`#[sea_orm(column_type = "JsonBinary")]`
- Optional 字段用 `Option<T>`，无 serde default；应用代码通过 `unwrap_or()` 或 `_or_default()` 方法处理

### Clippy 全局 Lint

在每个 crate root 文件（lib.rs / main.rs）添加：

```rust
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]
```

特定位置可添加局部 `#[allow(clippy::...)]`。

### Logging 约定

| 级别       | 用途                |
|----------|-------------------|
| `trace!` | 权限检查入口/退出、DB 查询细节 |
| `debug!` | 步骤完成、缓存命中、权限通过    |
| `info!`  | 启动、缓存初始化、配置重载     |
| `warn!`  | 权限拒绝、验证失败、锁中毒恢复   |
| `error!` | DB 连接失败、RPC 请求失败  |

Tracing target 遵循 crate/领域名：

| Target                                 | Crate                               |
|----------------------------------------|-------------------------------------|
| `server`                               | server                              |
| `rpc`                                  | ng-infra（`rpc_exec!` 宏，跨切面）         |
| `cache`                                | ng-infra（`make_global_cache!` 宏）    |
| `kv`                                   | ng-kv                               |
| `token`                                | ng-token                            |
| `token_cache`                          | ng-token/cache                      |
| `static_bucket` / `static_bucket_file` | ng-static                           |
| `terminal`                             | ng-terminal                         |
| `js_worker` / `js_result`              | ng-js-worker                        |
| `js_runtime`                           | ng-js-runtime                       |
| `monitoring`                           | ng-monitoring                       |
| `crontab` / `crontab_result`           | ng-crontab                          |
| `db`                                   | ng-db（虚拟 target，自动展开为 sea_orm/sqlx） |

### 命名约定

- 函数：`snake_case`，动词开头：`check_kv_read_permission`, `get_v_from_kv`
- 类型/结构体：`PascalCase`：`KVStore`, `TokenCache`
- 常量：`SCREAMING_SNAKE_CASE`：`NAMESPACE_MARKER_KEY`, `DEFAULT_CONNECT_TIMEOUT_MS`
- 枚举变体：`PascalCase`：`NodegetError::PermissionDenied`
- Crate 到命名空间映射：`ng-kv` → `kv`，`ng-token` → `token`，`ng-js-worker` → `js-worker`

### 模块组织

**Crate root** (`lib.rs`) 标准模板：

```rust
//! Crate doc: ## Default features / ## `server` feature

mod always_available_module;
pub use always_available_module::PublicType;

#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
pub mod rpc;

#[cfg(feature = "server")]
pub use auth::{TokenPermissionChecker, set_token_checker};
```

**RPC 目录结构**：每个方法一个文件 + `mod.rs`（trait + impl + `rpc_module()`）：

```
src/rpc/
  mod.rs           -- #[rpc] trait + impl + rpc_module()
  create.rs        -- handler
  read.rs
  update.rs
  delete.rs
```

子命名空间用子目录：

```
src/rpc/
  mod.rs
  static_bucket/
    mod.rs, auth.rs, create.rs, ...
  static_bucket_file/
    mod.rs, auth.rs, upload_file.rs, ...
```

### SeaORM Entity 约定

- 由 `sea-orm-codegen` 自动生成，每个表一个文件
- JSON 列标注 `#[sea_orm(column_type = "JsonBinary")]`
- 主键统一 `id: i64` + `#[sea_orm(primary_key)]`
- ActiveModel 构造使用 `Set()` + `..Default::default()`
- 修改 Entity 前先运行 migration，再执行 sea-orm-cli 生成实体（输出路径需指向 `crates/ng-db/src/entity`）：
  ```bash
  sea-orm-cli generate entity \
      -u "sqlite://test.db?mode=rwc" \
      -o crates/ng-db/src/entity \
      --with-serde both
  ```

### 测试约定

- 使用 `#[cfg(test)] mod tests` 内联于源文件
- `use super::*` 导入
- 标准库 `#[test]` + `assert_eq!` / `assert!`，无外部测试框架
- 文件系统测试使用 `unique_tempdir()` 创建临时目录
- 运行：`cargo test --workspace`

---

## 新增功能 Checklist

添加新的 RPC 命名空间或方法时，需完成以下步骤：

1. **Migration**：在 `crates/ng-db/migration/` 添加新 migration step
2. **Entity**：运行 `sea-orm-codegen` 生成实体到 `crates/ng-db/src/entity/`
3. **Types**：在对应 crate 的 default feature 下添加类型定义和查询 DSL
4. **Auth**：在 crate 的 `auth.rs`（server feature）定义 `TokenPermissionChecker` trait + OnceLock 注入
5. **Handler**：在 `rpc/<method>.rs` 实现各方法，遵循四层结构
6. **Cache**（如需）：实现 `DbBackedCache` trait + `make_global_cache!`
7. **rpc_module**：在 crate `lib.rs` 导出 `rpc_module()`
8. **Merge**：在 `server/src/rpc_nodeget.rs::build_modules()` 中 merge 新 module
9. **Inject**：在 `server/src/subcommands/serve.rs` 中注册 trait 实现和缓存初始化
10. **Router**（如需）：实现 `pub fn router() -> axum::Router`，在 `serve.rs` 中 `.merge()`
11. **Doc**：在 `docs/api/` 添加对应的 VitePress 文档
12. **Config**（如需）：在 `ng-config` 添加配置字段 + 更新 `docs/guide/config/`

# NodeGet 代码注释规范

本文件定义了项目统一的注释风格，所有模块注释必须遵循此规范。

## 1. 模块级注释 (`//!`)

每个 `.rs` 文件顶部必须有模块级注释，说明：

- 模块用途（一句话概括）
- 核心职责
- 与其他模块的协作关系（如适用）

```rust
//! 监控数据缓冲区
//!
//! 负责收集 Agent 上报的监控数据，批量写入数据库。
//! 与 MonitoringLastCache / MonitoringUuidCache 协作，提供查询服务。
```

## 2. 函数/方法级注释 (`///`)

每个 **pub** 函数必须有文档注释，包含：

- 功能简述（一句话）
- 参数说明：`- ` 开头，每个参数一行
- 返回值说明
- 内部步骤：编号列表 `1. 2. 3.`，描述关键流程

```rust
/// 查询监控数据
///
/// - `token`: 认证 Token
/// - `query`: 查询条件
/// - 返回: 符合条件的监控数据列表
///
/// 1. 验证 Token 权限
/// 2. 从缓存或数据库查询数据
/// 3. 过滤无权限的 UUID
```

非 pub 但逻辑复杂的函数也应添加注释，格式相同。

## 3. 行内注释 (`//`)

以下场景必须有行内注释：

- 复杂算法/数学运算
- 不直观的排序/过滤/转换逻辑
- 性能优化相关的特殊处理（如锁的提前释放、批量操作）
- 安全相关的校验逻辑
- 注释回答 **"为什么"** 而非 **"是什么"**
- 不加显而易见的注释（如 `let x = 5; // x 等于 5`）

```rust
let timestamp_ms = utc.timestamp_millis_opt(t)
.single()
.unwrap_or_else(| | {
// 非唯一时间戳（罕见），使用第一个候选值作为降级策略
warn ! ("非唯一时间戳: {t}");
utc.timestamp_millis_opt(t).earliest().unwrap()
});
```

## 4. 结构体/枚举注释 (`///`)

- 结构体本身：说明用途
- 每个字段：说明含义，标明单位（如毫秒时间戳、字节数）

```rust
/// Crontab 缓存条目
struct CachedCrontab {
    /// 数据库主键 ID
    id: i64,
    /// 关联的 Agent UUID
    agent_uuid: String,
    /// 上次执行时间（毫秒时间戳）
    last_run_time: i64,
}
```

## 5. 风格规则

- **中文为主**，技术术语保留英文：Token, RPC, Cache, RwLock, WebSocket, UUID, Cron, Task 等
- 公开 API 用 `///` 文档注释
- 行内解释用 `//`
- 统一使用全角标点：，。；：
- 已有注释风格不统一的，统一到本规范
- 保持简洁，避免冗余；一行能说清的不写两行

## 6. 禁止修改的内容

- 不修改任何实际代码逻辑
- 不修改函数签名
- 不修改类型定义的结构（仅添加注释）
- 不修改 import 语句
- auto-generated 文件（entity/, migration/, build.rs）跳过

## 7. 跳过规则

以下文件无需添加注释：

- `ng-db/src/entity/` — SeaORM 自动生成
- `ng-db/migration/` — 迁移脚本自动生成
- `build.rs` — 构建脚本通常为自动生成
- 纯 re-export 的 `mod.rs`/`prelude.rs` — 仅当内容为简单 pub use 时可跳过
