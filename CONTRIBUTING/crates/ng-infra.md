# ng-infra — 基础设施抽象与 server 侧缓存 / RPC 工具层

> **概览**：NodeGet 的基础设施层。默认 feature 只导出轻量、Agent-safe 的抽象类型（`ScopedPermission<T>`、`PermissionResolver`、`RpcDispatcher`）；其中 `PermissionResolver` 与 `RpcDispatcher` 当前在仓库内主要处于“定义但未形成主运行路径”的状态。启用 `server` feature 后，crate 追加 DB 全表缓存与 RPC 工具：`DbBackedCache` trait + `make_global_cache!` 宏、`rpc_exec!` 日志宏、`token_identity`、`TruncatedRaw` 与 `RpcHelper`。它本身不是 server binary 的“组合根”，而是被多个服务端业务 crate 复用的基础设施库。

## 模块结构

| 文件 | 角色 |
|------|------|
| `src/lib.rs` | Crate 根。声明常驻 public 模块 `dispatcher`、`permission`，条件声明 `server` 模块（`feature="server"`），并 re-export 三个默认可见的公共项。确立 default-vs-server 的 feature 切分。 |
| `src/dispatcher.rs` | RPC 命名空间组装的轻量抽象层。定义 `RpcDispatcher` trait（单一方法 `merge`）；注释以“包装 jsonrpsee 的 `RpcModule`”为示例，但当前仓库未提供实现，server 二进制也未使用它，`server/src/rpc_nodeget.rs` 直接合并 `jsonrpsee::RpcModule<()>`。 |
| `src/permission.rs` | 框架无关的权限词汇表：泛型枚举 `ScopedPermission<T>`（All | Scoped(Vec<T>)）及其辅助方法，以及将 `(Token, Permission)` 映射为有效 `ScopedPermission<Scope>` 的 `PermissionResolver` trait。当前仓库仅定义该 trait，未见具体实现。无 DB/RPC 依赖，Agent 安全。 |
| `src/server.rs` | **仅 server feature**。`DbBackedCache` trait + `make_global_cache!` 宏（全表 OnceLock 缓存单例模式）、`load_from_db` 辅助函数、`token_identity` 解析器、`TruncatedRaw` Display 包装器、`rpc_exec!` 日志宏、`RpcHelper` trait（DB 访问 + JSON-Set 辅助）。被持有全表 DB 缓存的业务 crate 复用。 |

## 公共 API

### 默认 feature（Agent-safe）

| 名称 | 签名 | 行为 |
|------|------|------|
| `ScopedPermission<T>` | `pub enum ScopedPermission<T> { #[default] All, Scoped(Vec<T>) }`（derives Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize；`#[serde(rename_all = "snake_case")]` → `"all"` / `"scoped"`） | 泛型 `T: Eq`（通常实例化为 `ScopedPermission<Scope>`）。当前实现存储 `Vec<T>` 并通过 `Vec::contains` 检查，因此查找为 O(n)；`ng-core` 的 `Scope` 现在已实现 `Hash`，但这里仍沿用 `Vec` 表示。 |
| `ScopedPermission::is_allowed` | `pub fn is_allowed(&self, item: &T) -> bool` | `All` 无条件 `true`；`Scoped` 走 `Vec::contains`（O(n) 线性扫描）。 |
| `ScopedPermission::is_all` | `pub const fn is_all(&self) -> bool` | `matches!(self, Self::All)`，常量。 |
| `ScopedPermission::as_scoped` | `pub fn as_scoped(&self) -> Option<&[T]>` | `All => None`；`Scoped => Some(items.as_slice())`。 |
| `PermissionResolver` | `pub trait PermissionResolver: Send + Sync { fn resolve(&self, token: &Token, permission: &Permission) -> ScopedPermission<Scope>; }` | 输入借用，返回 owned `ScopedPermission`。当前仓库仅定义并 re-export 该 trait，未见 in-tree 实现或调用；绑定 ng-core 的 `Token` / `Permission` / `Scope`（`permission.rs:60`）。 |
| `RpcDispatcher` | `pub trait RpcDispatcher: Send + Sync + Sized { fn merge(&mut self, other: Self) -> anyhow::Result<()>; }` | `Sized` + 同类型 `merge`。注释以包裹 `jsonrpsee::RpcModule` 为典型场景，但当前仓库未见实现；server 端实际直接使用 `jsonrpsee::RpcModule<()>` 合并模块。 |

### `server` feature 独占

| 名称 | 签名 | 行为 |
|------|------|------|
| `load_from_db<E>` | `pub async fn load_from_db<E>() -> anyhow::Result<Vec<E::Model>> where E: EntityTrait + Send + Sync, E::Model: ModelTrait + Clone + Send + Sync + 'static` | 经 `ng_db::get_db()` 取全局 DB；None 时返回 `NodegetError::ConfigNotFound`；运行 `E::find().all(db).await`，将 sea-orm 错误映射为 `anyhow!`。 |
| `DbBackedCache` | `#[allow(async_fn_in_trait)] pub trait DbBackedCache: Sized + Send + Sync { type Model: ModelTrait + Clone + Send + Sync + 'static; fn cache_name() -> &'static str; fn build_cache(models: Vec<Self::Model>) -> Self; async fn reload_from_models(&self, models: Vec<Self::Model>); fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send; }` | `cache_name` 仅用于日志；`build_cache` 构造首次实例；`reload_from_models` 以 `&self` 交换状态（实现者必须内部加锁，因 OnceLock 只给共享引用）；`load_all` 一般委派给 `load_from_db::<Entity>()`。 |
| `make_global_cache!` | `#[macro_export] macro_rules! make_global_cache { ($ty:ty, $global:ident) => { ... } }` | 在调用点展开为 `static $global: OnceLock<$ty> = OnceLock::new();` 并向 `impl $ty` 注入 `init / global / reload`。宏只在启用 `ng-infra/server` 时可见，且调用 crate 需直接依赖 `tracing`。 |
| `token_identity` | `pub fn token_identity(token: &str) -> (&str, &str)` | 零分配解析：含 `:` → `(前缀, "")`（Token 模式）；否则含 `|` → `("", 前缀)`（Auth 模式）；否则 `("???", "")`。返回借用切片。 |
| `TruncatedRaw<'a>` | `pub struct TruncatedRaw<'a>(pub &'a RawValue); impl fmt::Display for TruncatedRaw<'_>` | `len() <= 1024` 时原样输出 `RawValue::get()`；否则在 `floor_char_boundary(1024)` 处截断并追加 `"[...{N} bytes total]"`。 |
| `rpc_exec!` | `#[macro_export] macro_rules! rpc_exec { ($expr:expr) => {{ ... }} }` | 包裹返回 `Result<Box<RawValue>, _>` 的表达式：Ok 时 debug 日志截断响应，Err 时 error 日志；原样透传值。宏只在启用 `ng-infra/server` 时可见，且调用 crate 需直接依赖 `tracing`。 |
| `RpcHelper` | `pub trait RpcHelper { fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>>; fn get_db() -> anyhow::Result<&'static DatabaseConnection>; }` | 带默认方法的 helper trait（`impl RpcHelper for Foo {}` 即可引入方法）。`try_set_json` 经 `serde_json::to_value` 序列化并以 sea_orm `Set` 包装，serde 错误映射为 `NodegetError::SerializationError`；`get_db` 委派 `ng_db::get_db()`，None 时返回 `NodegetError::DatabaseError`。 |

## 关键类型与常量

- **`ScopedPermission<T>`** — `crates/ng-infra/src/permission.rs:19`。泛型 `T: Eq`，`#[serde(rename_all = "snake_case")]`；当前内部存储为 `Vec<T>`，`is_allowed` 走线性查找。`Scope` 已实现 `Hash`，但该容器选择尚未变更。
- **`PermissionResolver`** — `crates/ng-infra/src/permission.rs:60`。`Send + Sync`，方法 `resolve(&self, token: &Token, permission: &Permission) -> ScopedPermission<Scope>`；当前仓库未见实现。
- **`RpcDispatcher`** — `crates/ng-infra/src/dispatcher.rs:8`。`Send + Sync + Sized`，方法 `merge(&mut self, other: Self) -> anyhow::Result<()>`；当前仓库未见实现或使用。
- **`DbBackedCache`** — `crates/ng-infra/src/server.rs:61`。`#[allow(async_fn_in_trait)]`；关联类型 `type Model: ModelTrait + Clone + Send + Sync + 'static`。
- **`make_global_cache!`** — `crates/ng-infra/src/server.rs:94`。`#[macro_export]`；定义位于 `#[cfg(feature = "server")]` 模块中，宏体内部用 `$crate::server::DbBackedCache`。
- **`token_identity`** — `crates/ng-infra/src/server.rs:173`。位置型：先查 `:` 再查 `|`。
- **`TruncatedRaw<'a>`** — `crates/ng-infra/src/server.rs:186`。`pub` 字段，可 `TruncatedRaw(&raw)` 内联构造；`Display` 实现见 `server.rs:188`。
- **截断阈值**：**1024 字节**，写死（`server.rs:197`）。
- **错误回退哨兵**：token_identity 无法识别分隔符时返回 `("???", "")`。
- **`rpc_exec!`** — `crates/ng-infra/src/server.rs:216`。`#[macro_export]`；宏体直接调用 `tracing::debug!` / `tracing::error!`。
- **`RpcHelper`** — `crates/ng-infra/src/server.rs:242`。默认方法 helper trait；文件顶部混合导入 sea_orm 的 `ActiveValue`/`Set` 与 ng-core 的 `NodegetError`（`server.rs:17-18`）。

## 内部机制

### DbBackedCache + make_global_cache! 生命周期

使用流程（参考 `server.rs:54-57` 文档与 `62-81` trait、`94-162` 宏）：

1. 业务 crate 实现 `DbBackedCache`（定义 `Model`、`cache_name`、`build_cache`、`reload_from_models`，`load_all` 一般委派 `load_from_db::<Entity>()`）。
2. 模块作用域调用 `ng_infra::make_global_cache!(MyCache, MY_CACHE_GLOBAL);`，宏注入 `init/global/reload` 到 `impl MyCache`。
3. server 启动时 `serve.rs` 调用 `MyCache::init().await`：从 DB 加载、`build_cache` 构造、`OnceLock::set` 写入，并以 INFO 记录 `cache initialized` 与 count。
4. 后续更新调用 `MyCache::reload().await`：重新 `load_all`，经 `reload_from_models`（`&self` 内部可变性）交换状态，DEBUG 记录 `cache reloaded`。
5. 读路径走 `MyCache::global() -> Option<&'static Self>`。

CLAUDE.md 列出的实际消费者：TokenCache、CrontabCache、StaticCache、MonitoringUuidCache。

### init / reload 并发安全

`init()` 可并发或重复调用：若 `OnceLock::set` 返回 Err（已被先前 init 占用），则 `tracing::warn!(target:"cache", ..., "already initialized")` 并改调 `Self::reload()`（`server.rs:113-120`）。即第二次 `init()` 等价于 `reload()`，不会 panic、也不会 no-op。`reload()` 在全局未初始化时是 no-op（返回 Ok，`server.rs:145-147`），避免启动顺序问题。

### rpc_exec! 日志契约

每个 RPC 方法返回 `RpcResult<Box<RawValue>>`，方法体把内部调用包进 `rpc_exec!(inner(args).await)`。宏对 Result 模式匹配：Ok 时 `tracing::debug!(target:"rpc", response=%TruncatedRaw(&raw), "request completed")`（超 1024 字节截断并附 `[...N bytes total]`），Err 时 `tracing::error!(target:"rpc", error=%e, "request failed")`。值原样透传。CLAUDE.md 注明 timing middleware 会单独按级别记录请求耗时，故本宏刻意只记录结果。

### token_identity 解析优先级

对来自 `TokenOrAuth` 的原始凭证字符串：优先匹配首个 `:`（Token 模式 `key:secret` → `(key, "")`）；否则匹配首个 `|`（Auth 模式 `username|password` → `("", username)`）；都不含则 `("???", "")`。基于 `find` 返回的首次出现位置。因 `:` 优先，同时含 `:` 与 `|` 的字符串被当作 Token 模式。返回借用 `&str`，零分配（`server.rs:173-183`）。

### TruncatedRaw char-boundary 安全

`TruncatedRaw::fmt` 使用 `str::floor_char_boundary(MAX)`（MAX=1024）寻找 char-safe 切片边界后再截断，保证不会切断多字节 UTF-8。完整原始字节长度以 `[...{N} bytes total]` 追加（`server.rs:190-201`）。

### RpcHelper DB 访问路径

`RpcHelper::get_db()` 委派 `ng_db::get_db()`（SeaORM 全局单例），返回 `&'static DatabaseConnection` 或 `NodegetError::DatabaseError`。`load_from_db` 也用 `ng_db::get_db()`，但 None 映射为 `NodegetError::ConfigNotFound`。同一根因（DB 未初始化）在两个入口产生不同错误变体——匹配错误时需注意。

### 宏导出可见性

`make_global_cache!` 与 `rpc_exec!` 均为 `#[macro_export]`，但定义位于 `#[cfg(feature="server")] pub mod server` 中（`lib.rs:19-20`、`server.rs:93-95`、`server.rs:215-217`）。因此只有在编译了 `ng-infra/server` 时它们才会被导出；下游未启用该 feature 时，`ng_infra::make_global_cache!` / `ng_infra::rpc_exec!` 名字本身就不可用，而不是展开后才因 `$crate::server` 缺失而失败。两者宏体还直接引用 `tracing::...`，调用 crate 需直接依赖 `tracing`。

### 默认 feature 的 type-only 契约

`ScopedPermission`、`PermissionResolver`、`RpcDispatcher` 通过 `lib.rs:22-23` 的 `pub use` 随默认（无 feature）构建发布。它们不要求启用 `server` feature 才会引入的 sea-orm/ng-db/serde_json 依赖。当前 Agent 二进制并未依赖 ng-infra，但该 default-feature 形态对 Agent 侧是安全的；其中 `PermissionResolver` 与 `RpcDispatcher` 目前仍是仓库内未落地的轻量抽象。

### 宏内部卫生

宏内使用 `__`-前缀的局部标识（`__models`、`__count`、`__instance`、`__inst`）以避免与调用者作用域冲突（`server.rs:107`、`109`、`110`、`145`、`148`、`150`、`151`）。

## Crate 内部约定

- **Feature 切分**：`default = []` 为轻量公共项；`server` feature 引入 sea-orm/serde_json/ng-db。`lib.rs:19-20` 用 `#[cfg(feature = "server")]` 包裹 `pub mod server`。
- **宏导出**：`make_global_cache!`（`server.rs:94`）与 `rpc_exec!`（`server.rs:216`）在 `server` 模块编译时以 `#[macro_export]` 暴露到 crate 根，调用形如 `ng_infra::xxx!`。
- **宏内 `$crate` 路径**：内部统一用 `$crate::server::...`（`server.rs:108`、`111`、`116`、`123`、`149`、`154`、`223`），保证跨 crate 无需手动 path 导入。
- **宏调用方依赖**：`make_global_cache!` / `rpc_exec!` 的宏体直接使用 `tracing::...`，消费这些宏的 crate 需显式依赖 `tracing`。
- **日志 target**：`"cache"` 用于缓存生命周期（`server.rs:115`、`121`、`152`），`"rpc"` 用于 RPC 结果日志（`server.rs:221`、`229`）——区别于 kv/token/js_worker 等领域 target。
- **结构化字段**：tracing 字段用 `key=value`（`name=`、`count=`、`response=`、`error=`），不用 format 字符串。
- **Serde 命名**：`ScopedPermission` 用 `rename_all = "snake_case"`，线上变体 `all` / `scoped`（`permission.rs:20`）。
- **零分配**：`token_identity` 返回借用 `(&str, &str)` 切片而非 owned String（`server.rs:166` 文档）。
- **Helper trait，而非注入点**：`RpcHelper` 通过空 impl 暴露默认方法（`server.rs:236-242`），但它不是当前 server 的 OnceLock 注入点；`PermissionResolver` 与 `RpcDispatcher` 目前也未在仓库内落地为主运行路径中的注入点。
- **中文文档注释**：server.rs / permission.rs / dispatcher.rs 大量中文 doc 注释，遵循 CLAUDE.md 的 "Chinese comments" 约定。

## 注意事项与陷阱

- **`is_allowed` 为 O(n) 线性扫描**（`crates/ng-infra/src/permission.rs:34`）。允许列表大的场景会变热；虽然 `Scope` 已实现 `Hash`，但 `ScopedPermission` 当前仍存 `Vec`，查找成本不会自动改善。
- **`token_identity` 优先 `:` 后 `|`**（`crates/ng-infra/src/server.rs:174-182`）。Token 模式（`key:secret`）恒胜；维护者必须保证 Token 的 key/secret 与 Auth 的 username 严格遵循分隔符约定——username 一旦含 `:` 会被误判为 Token 模式。
- **`TruncatedRaw` 依赖 `str::floor_char_boundary(1024)`**（`crates/ng-infra/src/server.rs:197`），为已稳定 nightly API；截断阈值 1024 字节写死，超大 RawValue 在 RPC 日志里会被截断（刻意为之，但会隐藏部分调试细节）。
- **`make_global_cache!::init()` 重复调用静默回退 reload**（`crates/ng-infra/src/server.rs:106-128`）。若并发已初始化，init 走 reload 路径只打一条 warn；返回的 `Ok(())` 不区分首次与回退分支，调用者无法判断走了哪条。
- **DB 未初始化有两种错误变体**（`crates/ng-infra/src/server.rs:42-44` vs `258-260`）：`load_from_db` 抛 `ConfigNotFound`，`RpcHelper::get_db` 抛 `DatabaseError`。按变体匹配 missing-DB 时两者都要处理。
- **`DbBackedCache` 的 `#[allow(async_fn_in_trait)]`**（`crates/ng-infra/src/server.rs:61`）：`load_all` 的 `impl Future + Send` 受 trait 约束，但 `reload_from_models` 的 auto-future 除 `Self: Send + Sync` 外不继承额外 Send bound；跨多线程 runtime await 的实现者必须自行确保 future 为 Send。
- **`reload_from_models` 用 `&self` 非 `&mut self`**（`crates/ng-infra/src/server.rs:77`）：因全局缓存经 `OnceLock::get()` 只能拿到共享引用。每个 `DbBackedCache` 实现必须自行内部可变性（如 `RwLock`/`Mutex`），裸 `&self` 字段赋值不编译，忘加锁会在并发 reload 与读之间产生竞争。
- **`RpcDispatcher::merge` 要求 `other: Self`**（`crates/ng-infra/src/dispatcher.rs:12`）：不能合并异种 dispatcher 包装类型；当前仓库虽未使用该抽象，但这一类型约束本身仍在。
- **宏与 feature 必须配套**（`crates/ng-infra/src/lib.rs:19-20`）：`make_global_cache!` / `rpc_exec!` 只在启用 `ng-infra/server` 时导出；消费 crate 还需直接依赖 `tracing`，因为宏体在展开点使用 `tracing::...`。

## 依赖关系

ng-infra 在 workspace 内始终依赖 `ng-core`、`anyhow`、`serde`；`server` feature 下额外依赖 `ng-db`、`sea-orm`、`serde_json`。下游方向：当前在 `server` feature 下依赖 ng-infra 的业务 crate 包括 `ng-token`、`ng-crontab`、`ng-static`、`ng-monitoring`、`ng-kv`、`ng-js-worker` 等，用于复用 `DbBackedCache` + `make_global_cache!` 缓存单例模式、`rpc_exec!` 日志宏与 `RpcHelper`。ng-task 当前不依赖 ng-infra，而是继续使用 `ng_db::rpc::{RpcHelper, token_identity}` 与 `ng_db::rpc_exec`。Agent 二进制当前也不依赖 ng-infra；不过默认 feature 公开项保持 Agent-safe。`RpcDispatcher` 目前仅定义在 ng-infra 中，server 二进制的 `server/src/rpc_nodeget.rs::build_modules()` 直接合并 `jsonrpsee::RpcModule<()>`。
