# ng-token — RBAC 实现层

> 概览：ng-token 是 NodeGet 的 RBAC 实现层。在 `server` feature 下，它持有全表内存 token 缓存（`TokenCache`，基于 `DbBackedCache` + `make_global_cache!`），对 `key:secret` / `username|password` 凭据做常量时间认证，负责 super token（ID=1）的生成/轮换/校验、子 token 创建、scope+permission 覆盖判定（仅后缀通配），以及 `token_*` JSON-RPC 命名空间的 CRUD。未启用 `server` 时仅再导出 ng-core 的类型，供 agent 等 crate 以类型-only 方式依赖。所有其他 crate 的 `PermissionChecker` / `JsWorkerService` 等注入 trait 最终都委派到 ng-token 的 `get_token` / `check_token_limit` / `check_super_token`。

## 模块结构

```
crates/ng-token/src/
├── lib.rs              # Crate root：再导出 ng-core 类型；server feature 下暴露 hash 助手、模块与 rpc_module()
├── cache.rs            # 全表内存缓存 TokenCache + SUPER_TOKEN_GLOBAL，常量时间认证
├── generate_token.rs   # 子 token 创建（父 token 须为 super）
├── get.rs              # token 查询、RBAC 匹配、Limit 解析器
├── super_token.rs      # super token（ID=1）生命周期：生成、原子轮换、校验
└── rpc/
    ├── mod.rs          # Rpc trait 定义 + TokenRpcImpl + RpcServer 调度（span + rpc_exec!）
    ├── create.rs       # token_create
    ├── delete.rs       # token_delete
    ├── edit.rs         # token_edit
    ├── get.rs          # token_get
    ├── change_password.rs  # token_change_password
    ├── roll_token_secret.rs # token_roll_token_secret
    ├── list_all_tokens.rs  # token_list_all_tokens
    └── utils.rs        # find_target_token / extract_target_identifier 等共享工具
```

## 公共 API

| 名称 | 签名 | 行为 |
|------|------|------|
| 类型再导出 | `pub use ng_core::permission::data_structure::{Limit, Permission, Scope, Token}; pub use ng_core::permission::token_auth::TokenOrAuth; pub use ng_core::error::{NodegetError, anyhow_to_nodeget_error};` (`lib.rs:18-20`) | 未启用 `server` 时唯一可见的 pub 项，使 crate 对 agent 安全。 |
| `hash_string` | `pub fn hash_string(need_hash: &str) -> String` (`lib.rs:56`) | `SHA256(b"NODEGET" \|\| input)` 的十六进制（64 字符小写），用于 DB 存储。 |
| `hash_to_bytes` | `pub fn hash_to_bytes(need_hash: &str) -> [u8; 32]` (`lib.rs:68`) | 同上摘要但返回原始 32 字节，供 `ct_eq` 直接比较，省去 hex 解码。 |
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<rpc::TokenRpcImpl>` (`lib.rs:82`) | 由 `TokenRpcImpl.into_rpc()` 构建 token 命名空间 RPC 模块，server 启动时合并。 |
| `TokenCache` | `pub struct TokenCache; impl DbBackedCache for TokenCache;` | 全表 token 缓存单例。查询：`find_by_key` / `find_by_username` / `get_super_token` / `get_all`；认证：`authenticate`。生命周期由 `make_global_cache!` 生成 `init()` / `global()` / `reload()`（`cache.rs:82`）。 |
| `check_super_token` | `pub async fn check_super_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<bool>` (`super_token.rs:168`) | 常量时间 super-token 校验。缓存未初始化或 super 记录缺失返回 `Err`；任何不匹配返回 `Ok(false)`；全匹配返回 `Ok(true)`。 |
| `get_token` | `pub async fn get_token(token_or_auth: &TokenOrAuth) -> anyhow::Result<Token>` (`get.rs:42`) | 缓存+ct_eq 认证，返回不含 hash 字段的 `Token`；失败统一 `AUTH_FAILED_MESSAGE`。 |
| `get_token_by_key_or_username` | `pub async fn get_token_by_key_or_username(identifier: &str) -> anyhow::Result<Token>` (`get.rs:107`) | 无认证查询：先 `find_by_key` 再 `find_by_username`，缺失返回 `NotFound`。super-token 管理路径。 |
| `check_token_limit` | `pub async fn check_token_limit(token_or_auth: &TokenOrAuth, scopes: &[Scope], permissions: &[Permission]) -> anyhow::Result<bool>` (`get.rs:347`) | super token 短路 `true`；否则 `get_token` → 时间窗校验 → 每个 `(scope, perm)` 对都必须被覆盖。 |
| `check_limits_cover` | `pub fn check_limits_cover(limits: &[Limit], req_scope: &Scope, req_perm: &Permission) -> bool` (`get.rs:310`) | 纯内存覆盖判定：任一 `Limit` 的 scopes 覆盖 `req_scope` 且 permissions 覆盖 `req_perm` 即为真。 |
| `parse_token_limit_with_compat` | `pub fn parse_token_limit_with_compat(token_limit_value: serde_json::Value) -> anyhow::Result<Vec<Limit>>` (`get.rs:165`) | 前向兼容解析：先直接解析，失败则丢弃未知 `Permission` 变体后重试。 |
| `generate_and_store_token` | `pub async fn generate_and_store_token(father_token_or_auth: &TokenOrAuth, timestamp_from: Option<i64>, timestamp_to: Option<i64>, token_limit: Vec<Limit>, username: Option<String>, password: Option<String>) -> anyhow::Result<(String, String)>` (`generate_token.rs:35`) | 校验父凭据为 super token、用户名/密码配对、禁止字符，生成 key(16)/secret(32)，插入行（`version=1`），reload 缓存；返回明文 `(key, secret)`。 |
| `generate_super_token` | `pub async fn generate_super_token() -> anyhow::Result<Option<(String,String)>>` (`super_token.rs:66`) | 幂等首次生成 super token；已存在（`UniqueConstraintViolation`）返回 `None`。 |
| `roll_super_token` | `pub async fn roll_super_token() -> anyhow::Result<(String,String)>` (`super_token.rs:109`) | 事务内 `delete_by_id(1)` + 插入新行；失败回滚。返回新凭据。 |

## 关键类型与常量

### `CachedToken`（`cache.rs:30`）

```rust
pub struct CachedToken {
    model: Arc<token::Model>,
    parsed_limits: Arc<Vec<Limit>>,
    token_hash_bytes: [u8; 32],
    password_hash_bytes: Option<[u8; 32]>,
}
```

预解析、预解码，认证路径除 `Arc::clone` 外零分配；`token_limit` 与 `model` 用 `Arc` 包裹，便于零拷贝返回 `Token`。

### 缓存结构

- `struct TokenCacheInner { by_key: HashMap<String, Arc<CachedToken>>, by_username: HashMap<String, Arc<CachedToken>> }`（`cache.rs:43`）—— 全表加载后建立的双索引。
- `pub struct TokenCache { inner: RwLock<TokenCacheInner> }`（`cache.rs:54`）—— 实现 `DbBackedCache`；super token 单独存放于 `SUPER_TOKEN_GLOBAL` 以避免锁竞争。
- `static SUPER_TOKEN_GLOBAL: RwLock<Option<Arc<CachedToken>>> = RwLock::new(None)`（`cache.rs:90`）—— ID=1 独立锁，仅在 init/reload 写入，每次认证读取。
- `make_global_cache!(TokenCache, TOKEN_CACHE_GLOBAL)`（`cache.rs:82`）—— 生成 `TOKEN_CACHE_GLOBAL` OnceLock 单例与 `init()` / `global()` / `reload()`。

### 哈希与认证常量

- `const AUTH_FAILED_MESSAGE: &str = "Invalid credentials"`（`cache.rs:27`、`get.rs:27`）—— 统一失败消息，避免泄露是 key 还是 secret 错误。
- 固定盐 `b"NODEGET"`（`lib.rs:70`）—— token_secret 与 password 共享，无 per-entry salt。
- 凭据生成长度：`token_key = generate_random_string(16)`，`token_secret / password = generate_random_string(32)`（来自 `ng_core::utils`）。

### RPC 类型

- `#[rpc(server, namespace = "token")] pub trait Rpc`（`rpc/mod.rs:37`）—— 7 个方法，命名空间分隔符为 `_`，故 wire 名为 `token_get`、`token_create` 等。
- `pub struct TokenRpcImpl;`（`rpc/mod.rs:87`）—— 零字段实现体；`RpcServer` 实现委派到子模块函数。
- `impl RpcServer for TokenRpcImpl`（`rpc/mod.rs:96`）—— 每方法经 `token_identity(&cred)` 提取 `(token_key, username)` 用于日志，构造 `info_span!(target:"token", ...)`，再以 `rpc_exec!(submodule_fn(...).await)` 包 `.instrument(span)` 调度。

### Limit 解析辅助（`get.rs`）

- `fn drop_unknown_permissions(value) -> Value`（`get.rs:140`）—— 仅保留可反序列化为 `Permission` 的项，前向兼容。
- `fn wildcard_matches_pattern(value, pattern) -> bool`（`get.rs:189`）—— **仅后缀通配**：以 `*` 结尾则 `starts_with(prefix)`，否则精确相等；`a*b` 视为精确。
- `fn permission_matches(granted, required) -> bool`（`get.rs:203`）—— 先精确相等，再同变体同操作的通配（`Kv(R/W/D)`、`CrontabResult(R/D)`、`JsResult(R/D)`、`Task(C/R/W/D)`）；跨变体/跨操作永不匹配。
- `fn scope_matches(limit_scope, req_scope) -> bool`（`get.rs:250`）—— `Global` 覆盖一切；同类型 `Scope` 中 `JsWorker`/`StaticBucket`/`Db` 用通配，`AgentUuid`/`KvNamespace` 精确；跨类型永不匹配；非 `Global` 不覆盖 `Global`。

### 缓存内部辅助（`cache.rs`）

- `fn recover_read/recover_write(lock)`（`cache.rs:63`）—— 用 `unwrap_or_else(e.into_inner())` 从 poisoned lock 恢复，`tracing` target `"token_cache"`。
- `fn hex_to_bytes(hex_str) -> Option<[u8;32]>`（`cache.rs:394`）—— 手写零分配 hex 解析；拒绝长度 ≠ 64 或非法字符。
- `fn hex_nibble(b) -> Option<u8>`（`cache.rs:411`）—— 仅 `0-9/a-f/A-F`。
- `fn build_maps(all_tokens) -> (by_key, by_username, super_token)`（`cache.rs:168`）—— 预解析 `token_limit`（`parse_token_limit_with_compat`，失败回退空 `Vec`），`hex_to_bytes` 解码 hash（失败回退 `[0u8;32]` 防止 ct_eq 假匹配）；ID==1 记录同时插入 `by_key`/`by_username`（有 username 时）。

## 内部机制

### Cache 生命周期与 reload 契约

`TokenCache::global()` 返回 `make_global_cache!` 生成的 OnceLock 单例。init/reload 时 `build_maps` 加载全部 token 行，预解析 limits（`Arc<Vec<Limit>>`）并将 hex hash 预解码为 `[u8;32]`，使认证路径除 `Arc::clone` 外零分配。每次 mutating RPC（create / edit / delete / change_password / roll / generate_super / roll_super）后，handler 调用 `TokenCache::reload().await`；reload 错误以 error 级别记录但**绝不向 RPC 调用方传播**（DB 变更已成功）。

### Super token 锁分离

ID=1 super token 存放在专用静态 `SUPER_TOKEN_GLOBAL`（`cache.rs:90`）而非主 `inner` 锁，因为 `authenticate()` 几乎每个请求都要检查 super token。该锁仅 init/reload 写入（极低频），近乎无竞争。**关键顺序**：`reload_from_models`（`cache.rs:128`）**先**更新 `inner.by_key`/`by_username`，**再**更新 `SUPER_TOKEN_GLOBAL`。颠倒会制造一个窗口——过期 super 凭据仍能通过 `by_key` 认证但 `is_super=false`，即**权限降级窗口**；`cache.rs:130-135` 注释明确记录此约束。

### 常量时间认证流程

`check_super_token`（`super_token.rs:168`）先检查 `TokenCache::global().is_none()`，返回 `ConfigNotFound`（而非误导性的 `NotFound`）；再读 `get_super_token()`，对 Token 变体 ct_eq `(key, secret-hash)`、Auth 变体 ct_eq `(username, password-hash)`。部分不匹配返回 `Ok(false)`，全匹配返回 `Ok(true)`。`authenticate()`（`cache.rs:282`）镜像此流程并额外短路：当 key 匹配 super 但 secret 不匹配时**立即返回失败**，不再扫 `by_key`——因为 super key 全局唯一，重扫只会重命中同一 super 条目并多烧一次 SHA256；这让「错误 super secret」与「错误普通 secret」两条路径的 SHA256 次数一致。所有失败返回 `NodegetError::PermissionDenied("Invalid credentials")`。

### RBAC 匹配语义

`check_token_limit`：super token → 立即 `true`；否则 `get_token` 后用 `get_local_timestamp_ms_i64` 做时间窗校验（`now < timestamp_from` → false；`now > timestamp_to` → false）。随后对输入两个切片的**全笛卡尔积**中每个 `(req_scope, req_perm)` 对，`check_limits_cover` 必须成立；任一对未覆盖 → false。`check_limits_cover` 遍历 `limits`：某 limit 覆盖当且仅当其任一 scope `scope_matches req_scope` 且任一 permission `permission_matches req_perm`。

### 通配与 scope 匹配规则

仅后缀通配：以 `*` 结尾的模式匹配给定前缀的所有值（`strip_suffix('*')` 后 `starts_with`）；否则精确相等。`a*b` 视为精确（无中缀匹配）。`scope_matches`：`Global` 覆盖一切；`JsWorker`/`StaticBucket`/`Db` 用通配；`AgentUuid`/`KvNamespace` 精确相等；跨类型 scope 永不匹配；非 `Global` 不覆盖 `Global`。`permission_matches`：要求同变体+同操作；`Kv(R/W/D)`、`CrontabResult(R/D)`、`JsResult(R/D)`、`Task(C/R/W/D)` 对其字符串参数用通配；跨变体即使带 `*` 也永不匹配。

### Super token 生成的原子性

`roll_super_token`（`super_token.rs:109`）将 `delete_by_id(1)` + `insert(新 id=1)` 包在 `db.transaction` 内；insert 失败则 delete 回滚，防止 super token 丢失。`generate_super_token`（`super_token.rs:66`）幂等：尝试 insert，通过 `sql_err() == UniqueConstraintViolation`（**不是**错误字符串匹配，以存活 PostgreSQL 非英文 locale）判定为「已存在」，返回 `None`。

### 错误转换流水线

`rpc/*.rs` 中每个 RPC 方法遵循同一形态：内部 `let process_logic = async { ... anyhow::Result ... };`，外层 `match process_logic.await { Ok -> Ok, Err -> anyhow_to_nodeget_error -> ErrorObject::owned(code, msg, None::<()>) }`。`rpc/mod.rs` 的 `#[rpc]` + `RpcServer` 实现额外经 `rpc_exec!` 与 `token_identity` 包一层 tracing span。`token_identity` 从原始凭据串提取 `(token_key, username)`，仅用于日志关联。

### Trait 注入集成

ng-token 由 `ServerPermissionChecker`（在 `serve.rs` 注册）消费，后者将 `check_token_limit` / `check_super_token` / `get_token` 委派到本 crate。按 CLAUDE.md 的 trait 注入表，ng-crontab 的 `JsWorkerScheduler`、ng-task 的 `MonitoringUuidProvider`、ng-js-runtime 的 `JsWorkerServiceImpl` 最终都经这些函数完成鉴权。

## RPC 方法

命名空间 `token`（分隔符 `_`）。**所有方法的首个参数为调用方凭据串，handler 内部自行认证——框架层无鉴权**。每个方法都必须调用 `verify_supertoken` / `check_super_token`；新增方法若遗漏此检查则等同公开未鉴权。

| 命名空间 | 方法 | 参数 | 所需权限 | 行为 |
|----------|------|------|----------|------|
| `token` | `get` | `token: String`, `supertoken: Option<String>` | 自查：自身有效凭据；管理查询：super token | 无 supertoken：把 `token` 当凭据调 `get_token`。有 supertoken：先 `check_super_token`，再把 `token` 当凭据（`get_token`），解析失败则当 key/username 调 `get_token_by_key_or_username`。返回序列化 `Token`。 |
| `token` | `create` | `father_token: String`, `token_creation: TokenCreationRequest { timestamp_from: Option<i64>, timestamp_to: Option<i64>, token_limit: Vec<Limit>, username: Option<String>, password: Option<String> }` | super token | 委派 `generate_and_store_token`：super 校验、配对/禁字符校验、随机 key(16)/secret(32)、hash+insert（`version=1`）、reload。返回 `{"key":...,"secret":...}`。 |
| `token` | `delete` | `token: String`, `target_token: String` | super token | 拒绝空 target；拒绝 target 等于 super 的 key 或 username；先按 token_key 删除，回退 username；DB 过滤 `Id.ne(1)`。返回 `{"message":...,"rows_affected":N,"matched_by":"token_key\|username"}` 或 `NotFound`。 |
| `token` | `change_password` | `token: String`, `target_token: String`, `new_password: String` | super token | `new_password` 非空且 ≥ 6 字符；经 `utils::find_target_token` 定位，置 `password_hash=hash_string(new_password)`，更新 DB，reload。返回 `{"success":true,"message":"Password changed successfully"}`。 |
| `token` | `roll_token_secret` | `token: String`, `target_token: String` | super token | 定位目标，生成新 32 字符 secret，置 `token_hash=hash_string(new_secret)`，更新 DB，reload（旧 secret 立即失效）。返回 `{"key":token_key,"secret":new_secret}`。 |
| `token` | `list_all_tokens` | `token: String` | super token | 读 `TokenCache::global().get_all()`，将每个 `CachedToken` 映射为 `Token`（`Arc::clone parsed_limits`）。返回 `{"tokens":[...]}`。 |
| `token` | `edit` | `token: String`, `target_token: String`, `limit: Vec<Limit>` | super token | 先按 token_key 再按 username 定位；拒绝 `id==1`；以 `serde_json::to_value(limit)` **整体替换** `token_limit`（非合并）；更新 DB，reload。返回 `{"success":true,"id":N,"token_key":"..."}`。 |

### 鉴权流程

namespace 不受 jsonrpsee 框架保护。每个方法的首参（`token` / `father_token`）是调用方凭据，方法体内调用 `check_super_token`（管理类）或 `get_token`（自查）。新建方法必须显式调用其中之一，否则该方法是公开未鉴权的。

## 数据库实体

实体 `token` 定义在 ng-db（`crates/ng-db/src/entity/token.rs`），ng-token 经 `ng_db::entity::token` 引用。

| 表名 | 列 | 约束 / 索引 / 关系 | 备注 |
|------|------|---------------------|------|
| `token` | `id`（PK，super token 固定 1）；`version`（i32，创建时置 1）；`token_key`（String，unique）；`token_hash`（String，secret 的 SHA256+NODEGET hex）；`time_stamp_from`（Option<i64> ms epoch，None=立即）；`time_stamp_to`（Option<i64> ms epoch，None=永不）；`token_limit`（`Limit` JSON 数组，super 为 `[]`）；`username`（Option<String>，存在时唯一；super="root"）；`password_hash`（Option<String>，password 的 SHA256+NODEGET hex） | `id` PK；`token_key` unique；`username` unique when present | 凭据形式为 `token_key:token_secret` 或 `username\|password`。super token 行 `id=1` 且 `token_limit=[]`，由 `check_super_token` 快路径豁免所有限制——**非**特殊 DB 数据。delete 经 `.ne(1)` 排除 id=1；edit 显式拒绝 `id==1`。`token_hash` 与 `password_hash` 均为 `SHA256(b"NODEGET" \|\| raw)`。 |

## Crate 内部约定

- **Edition 2024**：全程使用 let-chains（如 `cache.rs:364`、`get.rs:364`）。
- **Feature gates**：`default = []` 仅再导出类型（agent 安全）；`server` feature 门控 `cache` / `generate_token` / `get` / `rpc` / `super_token` 模块、`hash_string` / `hash_to_bytes`，以及 `rpc_module()`。
- **自定义 jsonrpsee fork**：`#[rpc(server, namespace = "token")]` + `#[method(name="...")]`；分隔符 `_`，方法名为 `token_get` 等。
- **统一返回类型**：所有 RPC 方法返回 `RpcResult<Box<RawValue>>`；body 经 `serde_json::value::to_raw_value` 或 `RawValue::from_string` 构造原始 JSON。
- **统一错误转换**：每个 RPC 边界，内部 async 返回 `anyhow::Result`，外层经 `ng_core::error::anyhow_to_nodeget_error` 映射为 `jsonrpsee::types::ErrorObject::owned(code, msg, None::<()>)`。
- **日志 target 约定**：`"token"` 管理操作，`"auth"` 认证，`"token_cache"` 缓存锁/poison 事件。
- **哈希**：SHA256，前置固定盐 `b"NODEGET"`（`lib.rs:70`）；token_secret 与 password 同算法。DB 存 hex，缓存存原始 `[u8;32]`。
- **常量时间比较**：凡涉及秘密处都用 `subtle::ConstantTimeEq`（`ct_eq`）：token_key、username、token_hash、password_hash。
- **中文文档注释**：crate 级与函数级均为中文；测试名与少量行内注释为英文。
- **缓存模式**：`DbBackedCache` + `make_global_cache!` 生成 `TOKEN_CACHE_GLOBAL` OnceLock 与 `init()` / `global()` / `reload()`（`cache.rs:82`）。
- **全表加载 + reload 刷新**：任何 DB 变更后 RPC 方法调用 `TokenCache::reload().await`（错误记录不传播）。
- **Arc 包裹**：`CachedToken` 将 `token_limit` 包 `Arc<Vec<Limit>>`、`model` 包 `Arc<token::Model>`，使 `Token` 可经 `Arc::clone` 零拷贝返回。
- **Super token 约定**：ID=1，username="root"，`token_limit=[]`；经 `check_super_token` 快路径豁免所有限制。

## 注意事项与陷阱

- **维护者必须保持 `reload_from_models` 的更新顺序**（`cache.rs:128`）：先 `inner.by_key`/`by_username` 再 `SUPER_TOKEN_GLOBAL`。颠倒会产生权限降级窗口（过期 super 凭据经 `by_key` 通过但 `is_super=false`）。`cache.rs:130-135` 注释明确记录。
- **切勿在锁临界区内引入新的 panic 路径**（`cache.rs:63`）。poisoned RwLock 恢复用 `e.into_inner()`，单线程 panic 不会崩溃 server，但缓存可能服务过期/不一致数据。
- **切勿手工存储全零 hash**（`cache.rs:194`）。`build_maps` 对 hex 解码失败的行回退 `[0u8;32]`；若某 token 的 secret 摘要恰为全零，则可能用全零摘要通过认证。实际 SHA256 不会产生全零。
- **token_limit 损坏会静默丢权限**（`cache.rs:182`）。`parse_token_limit_with_compat` 失败时记录 warning 并替换为空 `Vec`——这是有意保活，但意味着数据损坏表现为权限丧失而非错误。
- **`check_token_limit` 是 AND 语义**（`get.rs:377`）：对 `scopes × permissions` 全笛卡尔积要求**每对**都被覆盖。传入长度不匹配切片 intending OR 语义会得到 AND 语义。
- **仅后缀通配**（`get.rs:189`）：`wildcard_matches_pattern` 只把**尾部** `*` 当通配；`a*b` 按精确匹配，中缀 `*` 被当字面量——常见意外。
- **用户名禁用 `:` 与 `|`**（`generate_token.rs:77`）：`TokenOrAuth::from_full_token` 以 `:`（Token 模式）和 `|`（Auth 模式）作分隔符。已迁移进 DB 的此类行将无法以 `username|password` 登录（非安全漏洞，是登录失败）；检查仅在创建时 fail-fast。
- **切勿把 super-token 存在检测改为错误字符串匹配**（`super_token.rs:66`）。`generate_super_token` 用 `sql_err() == UniqueConstraintViolation` 判定，以存活 PG 中文 locale；若 backend/SeaORM 版本不暴露 `SqlErr::UniqueConstraintViolation`，会返回 `DatabaseError` 而非 `None`，破坏幂等。
- **密码策略仅在 `change_password` 强制**（`rpc/change_password.rs:44`）：min 长度 6 且非空；`roll_token_secret` 无类似守卫。
- **edit 拒绝 `id==1` 是一致性守卫**（`rpc/edit.rs:83`）：尽管编辑 super 的 limit 无功能效果（super 经 `check_super_token` 绕过 limit），仍为与 delete 一致而硬拒。
- **`extract_target_identifier` 分隔符优先级**（`rpc/utils.rs:23`）：先按 `:` 再按 `|` 切分；`a:b|c` 得 `a`。优先级有意且已测试。
- **authenticate 短路依赖 token_key 全局唯一**（`cache.rs:305`）：super key 匹配但 secret 不匹配时直接失败、不扫 `by_key`。若未来 schema 允许重复 token_key，此快路径会错误拒绝与 super 同 key 的有效普通 token。
- **NODEGET 盐为硬编码且 secret/password 共享**（`lib.rs:70`）：无 per-entry salt，相同 secret 产生相同 hash（彩虹表友好）。常量时间比较仅防时序，不防预计算。**改动盐值必须重新哈希所有现存凭据**。
- **新建 RPC 方法切勿遗漏鉴权**（`rpc/mod.rs:41`）：所有 token RPC 方法把调用方凭据作为首参、handler 内部认证，框架不保护该命名空间。每个方法必须调用 `verify_supertoken`/`check_super_token`；遗漏即公开未鉴权。

## 依赖关系

ng-token 位于 RBAC 的核心，向内依赖 ng-core（`Token` / `Limit` / `Scope` / `Permission` / `TokenOrAuth` 类型、`NodegetError` 与 `anyhow_to_nodeget_error`、`generate_random_string`、`get_local_timestamp_ms_i64` 等 utils）与 ng-infra（`DbBackedCache` trait 与 `make_global_cache!` 宏、`rpc_exec!`、`token_identity`），并在 `server` feature 下引用 ng-db 的 `token` 实体。向外，几乎所有 server 业务 crate 通过 trait 注入间接到此：server binary 的 `ServerPermissionChecker` 直接委派 `check_token_limit` / `check_super_token` / `get_token`，ng-crontab（`CronJsWorkerScheduler`）、ng-task（`TaskMonitoringUuidProvider`）、ng-js-runtime（`JsWorkerServiceImpl`）的鉴权最终也都经本 crate；agent 仅以 default feature 依赖其类型再导出。
