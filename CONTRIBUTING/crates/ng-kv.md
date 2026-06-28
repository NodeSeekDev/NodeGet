# ng-kv：命名空间作用域的键值存储

> 概览：ng-kv 提供一个基于单张 `kv` 数据库表的命名空间作用域键值存储——每条记录对应一个 key，通过 `namespace` 列区分逻辑命名空间。它在默认特性下仅暴露 `KVStore` 类型（agent 也可安全依赖）；启用 `server` 特性后追加 SeaORM CRUD、RBAC 权限层（read/write/delete/list 以 `Permission::Kv` + 请求 `Scope::KvNamespace(namespace)` 做校验，匹配时 `Scope::Global` 也覆盖；create 仅 SuperToken），以及 `kv_*` JSON-RPC 命名空间（8 个方法）。本 crate 无 DB 缓存层，每次操作直接落库。

## 模块结构

```
crates/ng-kv/src/
├── lib.rs                    # Crate 根：声明模块、按 server 特性条件再导出公共 API
├── kv.rs                     # KVStore 类型（唯一无门控、永远编译的模块，含单元测试）
├── db.rs                     # server：kv 表的 SeaORM CRUD 层
├── auth.rs                   # server：RBAC 权限校验层 + 输入校验
└── rpc/                      # server：kv JSON-RPC 命名空间
    ├── mod.rs                #   Rpc trait、KvRpcImpl、rpc_module() 入口、DTO
    ├── create.rs             #   kv_create
    ├── get_value.rs          #   kv_get_value
    ├── get_multi_value.rs    #   kv_get_multi_value（最复杂，含通配与缓存）
    ├── set_value.rs          #   kv_set_value
    ├── delete_key.rs         #   kv_delete_key
    ├── delete_namespace.rs   #   kv_delete_namespace
    ├── get_all_keys.rs       #   kv_get_all_keys
    └── list_all_namespace.rs #   kv_list_all_namespace
```

| 文件 | 角色 |
|------|------|
| `lib.rs` | 文档化 default vs server 特性拆分；声明常驻的 `kv` 模块，并条件性声明 `auth`、`db`、`rpc` 模块；再导出公共表面。 |
| `kv.rs` | 定义 `KVStore` 结构体（命名空间作用域的 key→JSON value 存储），含单元测试。 |
| `db.rs` | 基于 `kv` SeaORM 表的 CRUD 层；实现“单表多命名空间”模型：`namespace` 列为逻辑命名空间，特殊 marker 行标记命名空间已被创建；所有操作经由 `ng_db::get_db()` 全局单例。 |
| `auth.rs` | KV 操作的 RBAC 权限层；校验命名空间/key、一次性解析 token 状态，通过内存 `Limit` 匹配 `Permission::Kv` 与请求 `Scope::KvNamespace(namespace)` 完成 read/write/delete/list 校验；其中 `ng_token` 的 scope 匹配允许 `Scope::Global` 覆盖该请求 scope；super-token 与 token 获取委派给 `ng_token` + 注入的 `PermissionChecker`。 |
| `rpc/mod.rs` | 定义 kv RPC 命名空间：DTO（`NamespaceKeyItem`、`KvValueItem`）、`#[rpc]` 生成的 `Rpc` + `RpcServer`、`KvRpcImpl` 服务结构体、`rpc_module()` 入口；每个方法用 `token_key` + `username` 构造 tracing span。 |

## 公共 API

### 类型与常量（默认特性即可用）

| 名称 | 签名 | 行为 |
|------|------|------|
| `KVStore` | `pub struct KVStore { namespace: String, kv: HashMap<String, serde_json::Value> }`（derive `Serialize, Deserialize, Clone, Default, Debug`），位于 `crates/ng-kv/src/kv.rs:16` | 命名空间名 + 内部 map；字段私有，仅通过方法访问。 |
| `KVStore::new` | `pub fn new(namespace: impl Into<String>) -> Self`（`#[must_use]`，`kv.rs:32`） | 构造空 KVStore，不访问 DB。 |
| `KVStore::namespace` | `pub fn namespace(&self) -> &str`（`kv.rs:44`） | 借用命名空间名。 |
| `KVStore::get` | `pub fn get(&self, key: &str) -> Option<&Value>`（`kv.rs:56`） | 按键查找。 |
| `KVStore::set` | `pub fn set(&mut self, key: String, value: Value)`（`kv.rs:66`） | 插入（覆盖）。 |
| `KVStore::remove` | `pub fn remove(&mut self, key: &str) -> Option<Value>`（`kv.rs:77`） | 移除并返回旧值。 |
| `KVStore::contains_key` | `pub fn contains_key(&self, key: &str) -> bool`（`kv.rs:88`） | — |
| `KVStore::keys` | `pub fn keys(&self) -> Vec<String>`（`kv.rs:97`） | 克隆所有 key；顺序为 HashMap 迭代序（非确定）。 |
| `KVStore::values` | `pub fn values(&self) -> Vec<&Value>`（`kv.rs:106`） | 借用所有 value；顺序与 `keys()` 一致但不应依赖。 |
| `KVStore::len / is_empty` | `pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool`（`kv.rs:115`，均 `#[must_use]`） | — |
| `KVStore::clear` | `pub fn clear(&mut self)`（`kv.rs:130`） | 清空所有条目，保留命名空间名。 |
| `KVStore::inner` | `pub const fn inner(&self) -> &HashMap<String, Value>`（`kv.rs:138`） | 直接借用底层 map（被 `get_multi_value` 通配路径用于 `.keys()`）。 |
| `KVStore::inner_mut` | `pub const fn inner_mut(&mut self) -> &mut HashMap<String, Value>`（`kv.rs:147`） | 直接可变借用。 |

### DB 操作（`server` 特性，`db.rs`）

| 名称 | 签名 | 行为 |
|------|------|------|
| `NAMESPACE_MARKER_KEY` | `pub const NAMESPACE_MARKER_KEY: &str = "__nodeget_namespace_marker__"`（`db.rs:22`） | `create_kv` 写入的标记键（值为 JSON `null`，存于 NOT NULL JSON/JSONB 列），用于标记命名空间已存在。**保留键名，勿用作真实 key**。 |
| `create_kv` | `pub async fn create_kv(namespace: String) -> Result<KVStore>`（`db.rs:63`） | 命名空间已存在则报 `DatabaseError`；否则插入 marker 行，返回空 KVStore。 |
| `get_v_from_kv` | `pub async fn get_v_from_kv(namespace: String, key: String) -> Result<Option<Value>>`（`db.rs:124`） | 调 `ensure_namespace_exists`（命名空间缺失则报错），按 `(namespace, key)` 单行查询。 |
| `get_v_from_kv_lenient` | `pub async fn get_v_from_kv_lenient(namespace: &str, key: &str) -> Result<Option<Value>>`（`db.rs:104`） | **不**确保命名空间存在；缺失返回 `None`。被 `get_multi_value` 精确键快速路径使用。**未在 `lib.rs` 再导出，仅 crate 内可见**。 |
| `set_v_to_kv` | `pub async fn set_v_to_kv(namespace: String, key: String, value: Value) -> Result<()>`（`db.rs:152`） | 调 `ensure_namespace_exists` 后用 `insert(...).on_conflict(OnConflict::columns([Namespace, Key]).update_column(Value))` upsert；**不会**隐式创建命名空间。 |
| `get_or_create_kv` | `pub async fn get_or_create_kv(namespace: String) -> Result<KVStore>`（`db.rs:185`） | 存在则 `get_kv_store`，否则 `create_kv`。 |
| `delete_key_from_kv` | `pub async fn delete_key_from_kv(namespace: String, key: String) -> Result<()>`（`db.rs:205`） | 确保命名空间存在，按 namespace AND key `delete_many`。 |
| `delete_kv` | `pub async fn delete_kv(namespace: String) -> Result<()>`（`db.rs:226`） | 确保命名空间存在，按 namespace `delete_many`（含 marker 与该命名空间下所有 key）。 |
| `get_keys_from_kv` | `pub async fn get_keys_from_kv(namespace: String) -> Result<Vec<String>>`（`db.rs:247`） | `select_only().column(Key).filter(namespace).order_by_asc(Key)`；**包含** marker key。 |
| `get_kv_store` | `pub async fn get_kv_store(namespace: String) -> Result<KVStore>`（`db.rs:272`） | 载入该命名空间下全部行到 KVStore；含 marker 行为 key=`NAMESPACE_MARKER_KEY`→`Null`。命名空间缺失则报错。 |
| `get_kv_store_optional` | `pub async fn get_kv_store_optional(namespace: String) -> Result<Option<KVStore>>`（`db.rs:293`） | 命名空间不存在返回 `None`；否则同 `get_kv_store`。被 `get_multi_value` 通配路径使用。 |
| `list_all_namespaces` | `pub async fn list_all_namespaces() -> Result<Vec<String>>`（`db.rs:319`） | `select_only().distinct().column(Namespace).order_by_asc(Namespace)`；存在性由任一行（含 marker）推断。 |

### 权限校验（`server` 特性，`auth.rs`）

| 名称 | 签名 | 行为 |
|------|------|------|
| `validate_namespace` | `pub fn validate_namespace(namespace: &str) -> anyhow::Result<()>`（`auth.rs:41`） | 拒绝空或含 `*`。**注：`auth.rs` 内 pub 但 `lib.rs` 未再导出**。 |
| `validate_key` | `pub fn validate_key(key: &str) -> anyhow::Result<()>`（`auth.rs:63`） | 拒绝含 `*`；**允许**空字符串（测试 `validate_key_valid_empty` 确认）。 |
| `validate_key_pattern` | `pub fn validate_key_pattern(key: &str) -> anyhow::Result<()>`（`auth.rs:79`） | 拒绝空；若含 `*`，必须恰有一个且为后缀（如 `metadata_*` 或 `*`）。 |
| `check_kv_read_permission` | `pub async fn check_kv_read_permission(token, namespace, key) -> Result<()>`（`auth.rs:171`） | `validate_namespace`+`validate_key`，解析 token；非 super 需有覆盖 `Scope::KvNamespace(namespace)` 请求的 `Kv::Read("*")` 或 `Kv::Read(key)`，其中 `Scope::Global` 也可覆盖该请求。 |
| `check_kv_read_permission_with_pattern` | `pub async fn ...(token, namespace, key_pattern) -> Result<()>`（`auth.rs:225`） | 同上但用 `validate_key_pattern`，允许后缀通配。 |
| `check_kv_write_permission` | `pub async fn ...(token, namespace, key) -> Result<()>`（`auth.rs:277`） | 需有覆盖 `Scope::KvNamespace(namespace)` 请求的 `Kv::Write("*")` 或 `Kv::Write(key)`；`Scope::Global` 也可覆盖。 |
| `check_kv_delete_permission` | `pub async fn ...(token, namespace, key) -> Result<()>`（`auth.rs:330`） | 需有覆盖 `Scope::KvNamespace(namespace)` 请求的 `Kv::Delete("*")` 或 `Kv::Delete(key)`；`Scope::Global` 也可覆盖。 |
| `check_kv_delete_namespace_permission` | `pub async fn ...(token, namespace) -> Result<()>`（`auth.rs:377`） | 需 `Kv::Delete("*")` 覆盖该命名空间删除请求；可由 `Scope::KvNamespace(namespace)` 或 `Scope::Global` 满足。逐 key 删除权限不足以删整个命名空间。 |
| `check_kv_list_keys_permission` | `pub async fn ...(token, namespace) -> Result<()>`（`auth.rs:426`） | 需 `Permission::Kv(Kv::ListAllKeys)` 覆盖该命名空间列举请求；通常请求 scope 为 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可覆盖；无通配替代。 |
| `resolve_kv_list_namespace_permission` | `pub async fn ...(token) -> Result<KvNamespaceListPermission>`（`auth.rs:469`） | 用 `get_checker()`：super→`All`；任一 limit 有 `Kv::ListAllNamespace` + `Scope::Global`→`All`，否则收集 `Scope::KvNamespace` 名为 `Scoped(set)`，无则 `PermissionDenied`。 |
| `check_kv_create_permission` | `pub async fn ...(token) -> Result<()>`（`auth.rs:689`） | 用 `get_checker()`；仅 super token 通过，否则 `PermissionDenied "Only SuperToken can create KV namespace"`。 |

### RPC 入口（`server` 特性，`rpc/mod.rs`）

| 名称 | 签名 | 行为 |
|------|------|------|
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<KvRpcImpl>`（`rpc/mod.rs:202`） | 返回 `KvRpcImpl.into_rpc()`，server 二进制合并进顶级路由（`build_modules` in `server/src/rpc_nodeget.rs`）。 |
| `NamespaceKeyItem` | `pub struct { pub namespace: String, pub key: String }`（derive `Debug, Clone, Serialize, Deserialize`，`rpc/mod.rs:34`） | `get_multi_value` 请求项；`key` 可为精确键或后缀通配模式。 |
| `KvValueItem` | `pub struct { pub namespace: String, pub key: String, pub value: Value }`（`rpc/mod.rs:43`） | `get_multi_value` 响应项；精确键缺失时 `value` 为 null。 |

## 关键类型与常量

| 项 | 位置 | 说明 |
|----|------|------|
| `KVStore` | `crates/ng-kv/src/kv.rs:16` | 命名空间作用域的内存存储；`namespace: String` + `kv: HashMap<String, serde_json::Value>`；derive `Serialize/Deserialize/Clone/Default/Debug`；字段私有。 |
| `NAMESPACE_MARKER_KEY` | `crates/ng-kv/src/db.rs:22` | `"__nodeget_namespace_marker__"`，`create_kv` 写入的标记键（值为 JSON `null`，存于 NOT NULL JSON/JSONB 列），存在性推断依据。 |
| `KvNamespaceListPermission` | `crates/ng-kv/src/auth.rs:24` | `enum { All, Scoped(HashSet<String>) }`——`resolve_kv_list_namespace_permission` 返回类型。 |
| `KvTokenState` | `crates/ng-kv/src/auth.rs:109` | `enum { Granted, Denied, Token(Token) }`——内部辅助，避免重复完整 auth 往返；`Denied` 覆盖时间无效（`timestamp_from` 在未来或 `timestamp_to` 在过去）token。 |
| `KvRpcImpl` | `crates/ng-kv/src/rpc/mod.rs:110` | `pub struct KvRpcImpl; impl RpcHelper for KvRpcImpl {}`——绑定到 `ng-infra` 的 `RpcHelper`（DB 访问/特性）。 |
| `Rpc` trait | `crates/ng-kv/src/rpc/mod.rs:54` | `#[rpc(server, namespace = "kv")] pub trait Rpc`——声明 8 个方法，均返回 `RpcResult<Box<RawValue>>`；线名 `kv_create`、`kv_get_value` 等（自定义 fork 以 `_` 为分隔符）。 |
| `lib.rs` 模块门控 | `crates/ng-kv/src/lib.rs:11` | `mod kv; pub use kv::KVStore;`（常驻）。 |
| `#[cfg(feature = "server")]` 模块门 | `crates/ng-kv/src/lib.rs:14` | 条件声明 `auth`、`db`、`rpc`。 |
| `pub use auth::{...}` | `crates/ng-kv/src/lib.rs:21` | server 再导出权限/校验助手（**不含** `validate_namespace`；导出的校验函数为 `validate_key`、`validate_key_pattern`）。 |
| `pub use db::{...}` | `crates/ng-kv/src/lib.rs:29` | server 再导出 DB ops；**`get_v_from_kv_lenient` 未在列**。 |
| `pub use rpc::{...}` | `crates/ng-kv/src/lib.rs:35` | server 再导出 `KvValueItem`、`NamespaceKeyItem`、`rpc_module`。 |

## 内部机制

### 无缓存层（每请求落库）

ng-kv **不**使用 `DbBackedCache` / `make_global_cache!`；服务端无任何 KV 数据内存缓存。每次 RPC 调用都通过 `ng_db::get_db()` 命中 PostgreSQL/SQLite。唯一的请求内缓存是 `get_multi_value` 中针对通配路径的局部 `HashMap<String, KVStore>`（作用域仅单次调用）。

### 单表命名空间模型 + marker 行

命名空间是逻辑概念：存在性由 `kv` 表中该 namespace 的任一行（含 marker）推断。`create_kv` 写入 key=`NAMESPACE_MARKER_KEY`、value=JSON `null` 的行（底层列为 NOT NULL JSON/JSONB）。`set_v_to_kv` **不会**隐式创建命名空间——它调 `ensure_namespace_exists`，否则报错，借此防止 RBAC 通过隐式创建被绕过。marker **未**从 `get_keys_from_kv` / `get_kv_store` 输出中过滤（`get_all_keys` 响应会包含 `__nodeget_namespace_marker__`）。

### Upsert 经由 ON CONFLICT

`set_v_to_kv` 使用 `kv::Entity::insert(...).on_conflict(OnConflict::columns([Namespace, Key]).update_column(Value))`。依赖迁移 `m20260205_024306_create_kv` 创建的 `UNIQUE(namespace, key)` 索引。SQLite 上等价于 `INSERT ON CONFLICT(namespace, key) DO UPDATE SET value=excluded.value`。

### Token 解析：统一 PermissionChecker 与直接 ng_token 路径

`auth.rs` 当前混用两类实现路径：

- 多数 key/namespace 读写删检查（如 `check_kv_read_permission`、`check_kv_write_permission`、`check_kv_delete_permission`）先通过 `ng_token::check_super_token` / `ng_token::get_token` 解析凭据，再在内存里对 `Limit` 做 `check_limits_cover` 匹配；请求 scope 为 `Scope::KvNamespace(namespace)`，但 `ng_token::scope_matches` 允许 `Scope::Global` 覆盖它。
- `check_kv_create_permission` 与 `resolve_kv_list_namespace_permission` 使用 `require_permission_checker()`（来自 `ng-core`）走统一的 `PermissionChecker` 接口。

这两条路径最终都落到同一套 RBAC 数据与匹配逻辑，但在实现风格上并不统一；新增权限函数时必须明确选择其一。
### 每次权限校验仅一次 get_token 往返

`auth.rs` 从原先两次 `check_token_limit` 调用优化为一次 `get_token` + 内存 `check_limits_cover`。`KvTokenState::Denied` 覆盖时间无效 token（`timestamp_from` 在未来、`timestamp_to` 在过去），返回 `PermissionDenied` 而非暴露单独的 “expired” 变体。

### RPC handler 的错误映射

每个 RPC handler 体被包裹为 `let process_logic = async { ... }; match process_logic.await { Ok => Ok, Err => 经 anyhow_to_nodeget_error 映射为 jsonrpsee ErrorObject::owned(code, msg, None) }`，错误码取自 `NodegetError::error_code()`。此模式在 8 个 handler 文件中重复；`rpc/mod.rs` 边界的 `rpc_exec!` 宏再包一层用于日志。

### 每请求一个 tracing span

每个 `KvRpcImpl` 方法调 `token_identity(&token) -> (token_key, username)`，创建 `info_span!(target: "kv", "kv::<method>", token_key, username, ...fields)`，内部逻辑块在 `.instrument(span)` 内执行。ng-kv 内部的领域日志/DB 操作以 `debug!`/`warn!`/`error!` 输出，主要 target 为 `"kv"`（`auth.rs:138`、`auth.rs:144` 有两处 stray target `"auth"`，用于 token 有效性告警）；最外层 `rpc_exec!` 基础设施日志则使用 target `"rpc"`。

### get_multi_value 的通配 + 精确键优化

`get_multi_value` 复用一次调用内的 `HashMap<String, KVStore>`，使同一命名空间的多个通配请求只做一次全命名空间载入。精确键请求默认绕过缓存，除非该命名空间已被通配请求缓存（此时从内存读取以保持一致、避免额外查询）。

### 存在性检查避免载入 value 列

`namespace_exists` 用 `select_only().column(namespace).into_tuple::<String>().one()`；`get_keys_from_kv` 用 `select_only().column(key)`；`list_all_namespaces` 用 `select_only().distinct().column(namespace)`。这些查询避免载入可能很大的 value JSONB。

## RPC 方法

命名空间 `kv`（自定义 jsonrpsee fork，分隔符 `_`，故线名为 `kv_<method>`）。所有方法返回 `RpcResult<Box<RawValue>>`，经 `rpc_exec!` 宏统一日志。每个方法先经 `token_identity(&token)` 提取 `(token_key, username)` 构造 span。

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `create` | `token: String, namespace: String` | SuperToken only（`check_kv_create_permission`） | `create_kv(name)`（插入 marker 行）；返回序列化的空 KVStore。trait 参数名 `namespace`，内部函数名 `name`。 |
| `get_value` | `token, namespace, key` | `Kv::Read("*")` 或 `Kv::Read(key)` 覆盖该请求；请求 scope 为 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可满足 | `get_v_from_kv`；返回 JSON 值，key 缺失返回字面量 JSON `null`（**非错误**）。命名空间不存在则报错。 |
| `get_multi_value` | `token, namespace_key: Vec<NamespaceKeyItem>` | `Kv::Read` 覆盖各项读取请求；每项请求 scope 为对应的 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可满足；**全部项须通过，否则整体失败** | 拒绝空输入；逐项 `check_kv_read_permission_with_pattern`。精确键走 `get_v_from_kv_lenient` 单行查询（缺失为 null）；通配项每命名空间载一次 `get_kv_store_optional`（缓存于局部 map），按 `starts_with(prefix)` 过滤、排序、逐匹配产出 `KvValueItem`。请求顺序保留，通配匹配内排序。 |
| `set_value` | `token, namespace, key, value: Value` | `Kv::Write`（全局或特定 key）覆盖该请求；请求 scope 为 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可满足 | `set_v_to_kv` upsert（ON CONFLICT (namespace,key) DO UPDATE value）；返回 `{"success":true}`；命名空间须已存在。 |
| `delete_key` | `token, namespace, key` | `Kv::Delete`（全局或特定 key）覆盖该请求；请求 scope 为 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可满足 | `delete_key_from_kv`；返回 `{"success":true}`。 |
| `delete_namespace` | `token, namespace` | `Kv::Delete("*")` 覆盖该请求；可由 `Scope::KvNamespace(namespace)` 或 `Scope::Global` 满足 | `delete_kv`（删该命名空间全部行含 marker）；返回 `{"success":true}`。 |
| `get_all_keys` | `token, namespace` | `Kv::ListAllKeys` 覆盖该请求；请求 scope 为 `Scope::KvNamespace(namespace)`，`Scope::Global` 也可满足 | `get_keys_from_kv`；返回升序 `Vec<String>`，**含** `NAMESPACE_MARKER_KEY`。 |
| `list_all_namespace` | `token` | `Kv::ListAllNamespace` 于 `Scope::Global`（→ All）或 `Scope::KvNamespace`（→ Scoped）；否则 `PermissionDenied` | `resolve_kv_list_namespace_permission` 后 `list_all_namespaces`，按 All/Scoped 过滤返回。 |

认证流：每个方法第一个参数为 `token`，经 `token_identity` 抽取身份后，业务前先调对应 `check_kv_*` / `resolve_*` 权限函数；super-token 在权限函数内短路通过。

## 数据库实体 / 迁移

### 实体

| 表名 | 列 | 约束/索引/关系 | 备注 |
|------|----|-----------------|------|
| `kv` | `id: BigInteger`（PK，auto_increment）；`namespace: String NOT NULL`；`key: String NOT NULL`；`value: Json (JsonBinary) NOT NULL` | 见下迁移；Relation enum 为空（无 FK） | 实体位于 `crates/ng-db/src/entity/kv.rs`（sea-orm-codegen 2.0 生成）。单物理表承载所有命名空间；命名空间是 `namespace` 列区分的逻辑概念。`id=1` 在此表**无**特殊含义（super-token 逻辑在 `ng-token`）。 |

### 迁移序列

迁移 `m20260205_024306_create_kv`：

- 列：`id BIGINT PK AUTO_INCREMENT`；`namespace VARCHAR NOT NULL`；`key VARCHAR NOT NULL`；`value JSON/JSONB NOT NULL`。
- 索引：`idx-kv-namespace-key-unique UNIQUE(namespace, key)`——支撑 `set_v_to_kv` 的 `ON CONFLICT (namespace, key)` upsert；`idx-kv-namespace INDEX(namespace)`——支撑命名空间存在性检查与命名空间范围扫描。
- 可逆：`down()` 直接 drop 整张表。

## Crate 内部约定

- **特性门控**：`default = []` 主要暴露 `KVStore`（仅类型，agent 安全）；`server` 特性追加 `auth.rs`、`db.rs`、`rpc/`。`lib.rs` 经 `#[cfg(feature = "server")] pub use` 条件再导出 server 门控项。
- **RPC 注册**：所有方法用 `#[rpc(server, namespace = "kv")]` + `#[method(name = "...")]` 过程宏——**切勿**手工 `register_method`。生成的线名为 `kv_<method>`（自定义 jsonrpsee fork 以 `_` 为分隔符）。
- **统一返回**：每个 RPC 经 `rpc_exec!` 宏返回 `RpcResult<Box<RawValue>>` 以统一日志。
- **日志 target**：ng-kv 内部的领域日志主要为 `"kv"`（`auth.rs:138`、`auth.rs:144` 有两处 stray `"auth"` 用于 token 有效性告警）；最外层 `rpc_exec!` 基础设施日志为 `"rpc"`。级别：成功路径 `debug`，校验/权限拒绝 `warn`，`create_kv` 插入失败 `error`。
- **中文注释**：内联文档注释为中文（如 宽松版、命名空间、校验），编辑时保持一致。
- **派生约定**：`KVStore` derive `Serialize/Deserialize/Clone/Default/Debug`；`NamespaceKeyItem`/`KvValueItem` derive `Debug/Clone/Serialize/Deserialize`。
- **Serde 约定**：值为 `serde_json::Value` 序列化到 `RawValue`；缺失键返回 JSON `null` 字面量而非错误。
- **权限模型**：请求通常以 `Scope::KvNamespace(namespace)` + `Permission::Kv(Kv::<op>("*" or key))` 表示；`"*"` 为该操作的权限通配，而 `ng_token` 的 scope 匹配允许 `Scope::Global` 覆盖该请求 scope。
- **DB 查询**：SeaORM；存在性检查用 `select_only().column(namespace).into_tuple::<String>().one()` 以避免载入 value 列。

## 注意事项与陷阱

- **维护者必须**在客户端侧过滤 `__nodeget_namespace_marker__`：`kv_get_all_keys` 输出会包含此内部 sentinel，因为 `get_keys_from_kv` 未过滤 marker 行。`crates/ng-kv/src/rpc/get_all_keys.rs:29` + `crates/ng-kv/src/db.rs:247`。
- **切勿**期待 `get_multi_value` 的部分成功：其权限为 ALL-OR-NOTHING，任一项缺读权限则整批 `PermissionDenied`。`crates/ng-kv/src/rpc/get_multi_value.rs:59`。
- **切勿**让 `set_v_to_kv` 隐式创建命名空间：它调 `ensure_namespace_exists`，命名空间缺失即 `DatabaseError "Namespace 'X' not found"`。这是有意为之（防 RBAC 绕过），创建命名空间须先经 SuperToken 调 `kv_create`。`crates/ng-kv/src/db.rs:152`。
- **维护者必须**保留“删整个命名空间需 `Kv::Delete("*")`”的严格性：即便 token 对每个 key 都有 `Kv::Delete(specific_key)`，也无法删命名空间；该删除请求可由 `Scope::KvNamespace(namespace)` 或 `Scope::Global` 覆盖。`crates/ng-kv/src/auth.rs:377`。
- `kv_create` / `create_kv` **当前不会**在进入 DB 前校验 namespace：`check_kv_create_permission` 只检查是否为 SuperToken，而 `create_kv` 直接写入 marker 行。不要把“所有触碰 DB 的命名空间路径都会先 `validate_namespace`”当作现状；若未来需要该不变量，应先改代码再写文档。`crates/ng-kv/src/rpc/create.rs:26` + `crates/ng-kv/src/db.rs:63`。
- **切勿**假设 key 非空：`validate_key` 拒 `*` 但允许空串（测试 `validate_key_valid_empty` 确认有意为之）。`crates/ng-kv/src/auth.rs:63`。
- **维护者必须**在新增 KV 权限函数时择一选用 token 解析路径：`resolve_kv_list_namespace_permission` 与 `check_kv_create_permission` 用 `require_permission_checker()`（注入的 `PermissionChecker`，需在 `server/src/subcommands/serve.rs` 注册）；而 read/write/delete/list-keys 直接走 `ng_token::`，依赖 `ng-token`/`TokenCache` 初始化而非额外 trait 注册。`crates/ng-kv/src/auth.rs:469`。
- **切勿**用 `get_value` 的 null 区分“键缺失”与“键显式为 null”：缺失返回字面量 JSON `null` 而非错误，需另做存在性检查。`get_multi_value` 精确键同样语义。`crates/ng-kv/src/rpc/get_value.rs:38`。
- **维护者必须**显式再导出方可跨 crate 使用 `get_v_from_kv_lenient`：它在 `db.rs` 声明为 `pub`，但**未**列入 `lib.rs` 的 `pub use db::{...}`，故实际为 crate 内私有。`crates/ng-kv/src/db.rs:104`。
- **维护者必须**保持 marker 行不被无意删除：`list_all_namespaces` 由行存在性推断命名空间；若未来代码路径删某命名空间的全部行（含 marker），该命名空间会从列表中静默消失，即便未显式 `delete_namespace`。`crates/ng-kv/src/db.rs:319` + `crates/ng-kv/src/rpc/list_all_namespace.rs:27`。
- **维护者必须**保护 `UNIQUE(namespace, key)` 索引：`set_v_to_kv` 的 upsert 依赖它；若该索引被 drop 或未迁移，`ON CONFLICT` 不匹配将导致重复行。迁移 `m20260205_024306_create_kv` 创建该索引，**切勿** drop。`crates/ng-kv/src/db.rs:165`。
- **切勿**依赖通配匹配内的插入/扫描顺序：`get_multi_value` 请求顺序跨项保留，但单个通配请求内匹配键经 `sort_unstable` 按字典序输出。`crates/ng-kv/src/rpc/get_multi_value.rs:114-115`。

## 依赖关系

ng-kv 启用 `server` 特性后依赖：`ng-core`（`TokenOrAuth`、`Permission`、`Scope`、`Kv`、`PermissionChecker` trait、`require_permission_checker`、`NodegetError`/`error_code`、`get_local_timestamp_ms_i64`）、`ng-db`（`kv` 实体、`get_db()` 全局单例、`DatabaseConnection`、迁移）、`ng-token`（`check_super_token`、`get_token`）、`ng-infra`（`rpc_exec!` 宏、`RpcHelper`、`token_identity`）、以及 `jsonrpsee`、`sea-orm`、`serde`/`serde_json`、`tracing`、`anyhow`。默认特性下仅暴露 `KVStore`，agent 可仅依赖本 crate 的类型而不引入 server 门控。本 crate 被 server 二进制（`server/src/rpc_nodeget.rs::build_modules` 调 `rpc_module()`）与文档/工具链消费；agent 不启用其 `server` 特性。
