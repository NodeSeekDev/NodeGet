# 编码规范执行手册

> 本文是 NodeGet 的编码规范执行参考，展开 `CONTRIBUTING.md` 中的规范并补充源码中观察到的细节。新增/修改代码必须遵守。各 crate 的局部约定见 [`crates/<name>.md`](../crates/) 的「Crate 内部约定」章节。

## 1. 三层错误体系

| 层级 | 类型 | 使用场景 |
|------|------|----------|
| 领域错误 | `NodegetError` 枚举（12 变体，数字 code 101/102/103/104/105/106/107/108/999，其中 Parse/Serialization/IO 共享 101） | 构造可克隆、有 code 的业务错误，**始终**经 `.into()` 转 `anyhow::Error` |
| 内部传递 | `anyhow::Result<T>`（ng-core 导出为 `Result<T>`，**注意是 anyhow 不是 `Result<_, NodegetError>`**） | 所有内部函数返回类型 |
| RPC 边界 | `RpcResult<Box<RawValue>>` | 仅用于 RPC handler 函数签名 |

### RPC handler 标准错误桥接（强制）

```rust
pub async fn some_method(token: String, /* ... */) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 1. 权限检查
        check_permission(&token, ...).await?;
        // 2. 业务逻辑
        let data = do_something().await?;
        // 3. 序列化为 RawValue
        serde_json::value::to_raw_value(&data)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };
    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}
```

- **`process_logic` 内部统一用 `?` + anyhow**；错误映射只在最外层做一次。
- 错误码由 `NodegetError::error_code()` 决定（const fn，见 [`crates/ng-core.md`](../crates/ng-core.md)）。
- 切勿在 handler 里手写 `ErrorObject::owned` 的 code 常量——用 `error_code()`。

## 2. RPC 方法四层结构（强制）

每个 RPC 方法必须拆为四层：

**层 1 — Trait 定义**（`rpc/mod.rs`，`#[rpc]` 宏）：

```rust
#[rpc(server, namespace = "kv")]
pub trait Rpc {
    #[method(name = "get_value")]
    async fn get_value(&self, token: String, namespace: String, key: String)
                       -> RpcResult<Box<RawValue>>;
}
```

**层 2 — Impl + tracing span**（`rpc/mod.rs`，`token_identity` + `info_span!` + `rpc_exec!`）：

```rust
pub struct KvRpcImpl;
impl RpcHelper for KvRpcImpl {}

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

**层 3 — Handler 函数**（`rpc/<method>.rs`，独立文件）：实现层 1 的标准桥接模式。

**层 4 — 模块注册**（`rpc/mod.rs` 底部）：

```rust
pub fn rpc_module() -> jsonrpsee::RpcModule<KvRpcImpl> {
    KvRpcImpl.into_rpc()
}
```

### 硬性规则

- **只用 `#[rpc]` proc 宏**，**绝不**手写 `register_method` / `register_async_method`。
- **首个参数恒为 `token: String`**（凭据），handler 内部认证。框架不保护命名空间——**每个方法必须调用鉴权**，遗漏即公开未鉴权（见各 crate 的「注意事项与陷阱」）。
- **返回类型恒为 `RpcResult<Box<RawValue>>`**。
- **子命名空间用子目录**：`rpc/static_bucket/{mod,auth,create,...}.rs`、`rpc/static_bucket_file/{mod,auth,...}.rs`。
- **jsonrpsee 命名空间分隔符是 `_`**（自定义 fork），不是 `.`。

## 3. Feature 门控模式

所有业务 crate 统一：

```toml
[features]
default = []
server = ["dep:jsonrpsee", "dep:sea-orm", ...]  # 所有重依赖
```

- **`default`**：仅类型、数据结构、查询 DSL——agent 可安全依赖。
- **`server`**：RPC handler、DB 查询、缓存、缓冲区——**仅** server binary 启用。
- **例外**：`ng-core` 用 `for-server` / `for-agent`（均仅启用 `libc`），不是 `default=[]` / `server`。
- **Crate root 模板**（`lib.rs`）：
  ```rust
  //! Crate doc: ## 默认 feature / ## `server` feature
  mod always_available_module;
  pub use always_available_module::PublicType;
  #[cfg(feature = "server")] mod auth;
  #[cfg(feature = "server")] pub mod rpc;
  #[cfg(feature = "server")] pub use auth::{TokenPermissionChecker, set_token_checker};
  ```

## 4. 缓存模式

### 全量 DB 缓存（用宏）

```rust
impl DbBackedCache for TokenCache {
    type Model = token::Model;
    fn cache_name() -> &'static str { "token" }
    fn build_cache(models: Vec<Self::Model>) -> Self { ... }
    async fn reload_from_models(&self, models: Vec<Self::Model>) { ... }  // 注意 &self，非 &mut self
    async fn load_all() -> anyhow::Result<Vec<Self::Model>> { load_from_db::<token::Entity>().await }
}
make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL);
// 自动生成 init() / global() / reload()
```

现有 DB-backed 缓存（用宏）：`TokenCache`、`CrontabCache`、`StaticCache`、`MonitoringUuidCache`。详见 [`crates/ng-infra.md`](../crates/ng-infra.md) 与 [`topics/cross-cutting.md`](cross-cutting.md) 的「缓存框架」。

### 派生态内存缓存（手写 OnceLock）

`MonitoringLastCache`、`StaticHashCache` **不**用宏——它们持有派生/最近值而非全表加载，用手写 `static CACHE: OnceLock<...>` 单例。

### 缓存生命周期

- `init()`：server 启动时全量加载。
- `reload()`：热重载（`RELOAD_NOTIFY`）或写操作后调用——重新从 DB 全量加载。
- `set_*` 静默忽略重复初始化：`let _ = LOCK.set(val);`。
- `get_*` 未初始化时 panic：`.expect("... not initialized -- call set_* first")`。部分返回 `Option`（如 `ng_db::get_db()`）。

## 5. OnceLock 全局注入模式（打破循环依赖）

所有跨 crate 依赖通过 `OnceLock` + `set_*()` / `get_*()` 注入：

- `set_*` 静默忽略重复初始化（`let _ = LOCK.set(val);`）。
- `get_*` 未初始化时 panic（`.expect("... not initialized")`）。
- server binary 在 `serve.rs` **统一注册所有 trait 实现**。

现有注入点与 trait 列表见 [`topics/cross-cutting.md`](cross-cutting.md) 的「Trait 注入」。所有实现最终委托给 `ng_token` 函数。

## 6. Serde 约定

- 所有枚举/结构体：`#[serde(rename_all = "snake_case")]`。
- 小写枚举变体（如 `IpProvider`）：`#[serde(rename_all = "lowercase")]`。
- JSON 列：`#[sea_orm(column_type = "JsonBinary")]`。
- Optional 字段用 `Option<T>`，**无 serde default**；应用代码用 `unwrap_or()` / `_or_default()` 处理。
- `Token.token_limit: Arc<Vec<Limit>>`——ng-core 启用 serde `rc` feature，Arc 按值序列化。

## 7. 日志约定

| 级别 | 用途 |
|------|------|
| `trace!` | 权限检查入/出、DB 查询细节 |
| `debug!` | 步骤完成、缓存命中、权限通过 |
| `info!` | 启动、缓存初始化、配置重载 |
| `warn!` | 权限拒绝、验证失败、锁中毒恢复 |
| `error!` | DB 连接失败、RPC 请求失败 |

### Tracing target（按 crate/领域）

| Target | Crate |
|--------|-------|
| `server` | server |
| `rpc` | ng-infra（`rpc_exec!` 宏，跨切面） |
| `cache` | ng-infra（`make_global_cache!` 宏） |
| `kv` | ng-kv |
| `token` / `token_cache` | ng-token / ng-token::cache |
| `static_bucket` / `static_bucket_file` | ng-static |
| `terminal` | ng-terminal |
| `js_worker` / `js_result` | ng-js-worker |
| `js_runtime` | ng-js-runtime |
| `monitoring` | ng-monitoring |
| `crontab` / `crontab_result` | ng-crontab |
| `db` | ng-db（虚拟 target，展开为 sea_orm/sqlx） |
| `auth` | 跨 crate 权限校验辅助（如 `token_time_valid`） |

- `info_span!` 起始处用 `token_identity(&token)` 提取 `(token_key, username)` 作结构化字段，**避免明文 token 入日志**。
- **凭据绝不入 tracing**：server binary 的子命令（roll_super_token 等）用 `println!` 输出凭据，而非 tracing（tracing 会落盘/聚合）。

## 8. 命名约定

- 函数：`snake_case`，动词开头（`check_kv_read_permission`、`get_v_from_kv`）。
- 类型/结构体：`PascalCase`（`KVStore`、`TokenCache`）。
- 常量：`SCREAMING_SNAKE_CASE`（`NAMESPACE_MARKER_KEY`、`DEFAULT_CONNECT_TIMEOUT_MS`）。
- 枚举变体：`PascalCase`（`NodegetError::PermissionDenied`）。
- Crate → 命名空间映射：`ng-kv` → `kv`，`ng-token` → `token`，`ng-js-worker` → `js-worker`。

## 9. Clippy 全局 Lint

每个 crate root（`lib.rs` / `main.rs`）：

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

特定位置可局部 `#[allow(clippy::...)]`（如 js-worker crate 级 `allow(clippy::too_many_arguments)`）。提交前 `cargo fmt`（**勿**用 RustRover 等 IDE 自带格式化）。

## 10. 注释规范（中文为主）

- `//!` 模块级：每个 `.rs` 顶部——用途、核心职责、协作关系。
- `///` 文档注释：每个 pub 函数——功能简述、`-` 参数说明、返回值、编号内部步骤。
- `//` 行内：解释**为什么**（复杂算法、性能优化、安全校验），不写显而易见的注释。
- 结构体/枚举：说明用途；每个字段说明含义与单位（毫秒时间戳、字节数）。
- 风格：中文为主，技术术语保留英文；全角标点 `，。；：`；简洁，一行能说清不写两行。

## 11. 测试约定

- `#[cfg(test)] mod tests` 内联于源文件。
- `use super::*` 导入。
- 标准库 `#[test]` + `assert_eq!` / `assert!`，无外部测试框架。
- 文件系统测试用 `unique_tempdir()` 创建临时目录。
- 运行：`cargo test --workspace`。

## 12. SeaORM Entity 约定

- `sea-orm-codegen` 自动生成，每表一文件。
- JSON 列 `#[sea_orm(column_type = "JsonBinary")]`。
- 主键统一 `id: i64` + `#[sea_orm(primary_key)]`（`monitoring_uuid.id` 是 `i32` 例外）。
- ActiveModel 构造用 `Set()` + `..Default::default()`。
- 改 schema 流程：① 新增 migration ② 跑迁移 ③ `sea-orm-cli generate entity -o crates/ng-db/src/entity --with-serde both`。

## 13. 新增 RPC 命名空间 Checklist

1. **Migration**：`crates/ng-db/migration/` 加新 step。
2. **Entity**：跑 `sea-orm-codegen` 生成实体。
3. **Types**：crate 的 default feature 下加类型与查询 DSL。
4. **Auth**：crate 的 `auth.rs`（server feature）定义权限校验（委托 `permission_checker`）。
5. **Handler**：`rpc/<method>.rs` 实现各方法，遵循四层结构。
6. **Cache**（如需）：实现 `DbBackedCache` + `make_global_cache!`。
7. **rpc_module**：crate `lib.rs` 导出 `rpc_module()`。
8. **Merge**：`server/src/rpc_nodeget.rs::build_modules()` 合并新 module。
9. **Inject**：`server/src/subcommands/serve.rs` 注册 trait 实现与缓存初始化。
10. **Router**（如需）：实现 `pub fn router() -> axum::Router`，在 `serve.rs` `.merge()`。
11. **Doc**：`docs/api/` 加 VitePress 文档；`CONTRIBUTING/crates/<name>.md` 更新对应章节。
12. **Config**（如需）：`ng-config` 加字段 + 更新 `docs/guide/config/`。
