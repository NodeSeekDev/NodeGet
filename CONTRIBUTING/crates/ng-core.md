# ng-core — 工作区共享基础 Crate

> 概览：ng-core 是 NodeGet 工作区的共享基础 Crate，被 server 与 agent 两个二进制共同依赖。它提供统一的 `NodegetError` 枚举与数字错误码、RBAC 数据模型（`Scope`/`Permission`/`Limit`/`Token`/`TokenOrAuth`）、监控与 JS 结果查询类型、编译期注入的版本元数据、通用工具（NTP 校时、随机字符串/UUID、JSON RawValue 助手）、Trait 注入的 `PermissionChecker` 契约，以及完整的在线自更新流水线（版本比较、下载 URL、二进制替换与回滚、execv 重启）。Agent 安全性通过非标准的 `for-server` / `for-agent` feature 控制（区别于其他业务 Crate 的 `default=[]` / `server` 模式）。

## 模块结构

| 文件 | 角色 |
|------|------|
| `src/lib.rs` | Crate 根，声明全部子模块（始终编译：`error`、`js_result`、`monitoring`、`permission`、`self_update`、`utils`），定义 `NameValidator` trait |
| `src/error.rs` | `NodegetError` 枚举（12 变体）、`error_code()` 数字映射、`JsonError` 转换、`From<serde_json::Error>`/`From<io::Error>`、`Result<T>` 别名、anyhow 还原助手 |
| `src/utils/mod.rs` | 工具根：`version`/`uuid` 子模块（常驻），`error_message`/`server_json` 子模块（for-server only）；定义 `JsonError`、全局 NTP offset、时间戳助手、随机串生成 |
| `src/utils/version.rs` | 编译期注入的 `NodeGetVersion`（13 字段），缓存于 `OnceLock` |
| `src/utils/uuid.rs` | UUID v4 生成封装 |
| `src/utils/error_message.rs` | for-server：构建 RPC 错误响应为 `Value` 或 `Box<RawValue>`（`rpc_exec!` 消费） |
| `src/utils/server_json.rs` | for-server：零拷贝 RawValue 构造、字符串化 JSON 列原地解析、键重命名 |
| `src/permission/mod.rs` | RBAC 模块根：`create`、`data_structure`、`permission_checker`（gated）、`token_auth` |
| `src/permission/data_structure.rs` | 核心 RBAC 数据结构：`Token`、`Limit`、`Scope`、`Permission` 及 13 个子操作枚举 |
| `src/permission/create.rs` | Token 创建 RPC 的请求体 `TokenCreationRequest` |
| `src/permission/permission_checker.rs` | for-server：对象安全 async trait 与 `OnceLock` 全局注入（替代原先 6 个分散的 auth trait） |
| `src/permission/token_auth.rs` | 双模认证凭据 `TokenOrAuth`（key:secret 或 username|password） |
| `src/monitoring/mod.rs` | 监控模块根，声明 `query` 子模块 |
| `src/monitoring/query.rs` | 静态/动态监控维度的字段枚举与列名/JSON-key 映射 |
| `src/js_result/mod.rs` | JS 结果模块根，声明 `query` 子模块（Agent 安全） |
| `src/js_result/query.rs` | JS 结果查询条件枚举与 AND 组合请求体 |
| `src/self_update.rs` | 在线自更新：版本解析、按架构表构造下载 URL、二进制替换（备份+回滚）、execv 重启 |
| `build.rs` | 手写 `VERGEN_*` env（shell 调 git 与 `rustc -vV`），不依赖 vergen Crate |

## 公共 API

### 错误与 Result

| 名称 | 签名 | 行为 |
|------|------|------|
| `NodegetError` | `pub enum`（12 变体，各携 `String`） | thiserror::Error + Debug + Clone；Display 形如 `"Permission denied: {0}"` |
| `NodegetError::error_code` | `pub const fn error_code(&self) -> i128` (`error.rs:66`) | 数值映射：InvalidInput=108，PermissionDenied=102，DatabaseError=103，AgentConnectionError=104，NotFound=105，UuidNotFound=106，ConfigNotFound=107，Other=999，ParseError/SerializationError/IoError=101（**共享**） |
| `NodegetError::to_json_error` | `pub fn to_json_error(&self) -> crate::utils::JsonError` (`error.rs:84`) | `JsonError{error_id: error_code(), error_message: to_string()}` |
| `anyhow_to_nodeget_error` | `#[must_use] pub fn anyhow_to_nodeget_error(err: &anyhow::Error) -> NodegetError` (`error.rs:114`) | downcast 成功则 clone，否则 `Other(err.to_string())` |
| `Result<T>` | `pub type Result<T> = anyhow::Result<T>` (`error.rs:107`) | **注意：是 anyhow，不是 `Result<_, NodegetError>`** |
| `JsonError` | `pub struct { pub error_id: i128, pub error_message: String }` (`utils/mod.rs:25`) | RPC 错误负载标准形状，Serialize+Deserialize |

### 工具与时间戳（`utils`）

| 名称 | 签名 | 行为 |
|------|------|------|
| `set_ntp_offset_ms` | `pub fn set_ntp_offset_ms(offset_ms: i64)` (`utils/mod.rs:38`) | 存储本地与服务器时钟偏移（`Ordering::Relaxed`），由 agent 调用 |
| `get_local_timestamp_ms` | `pub fn get_local_timestamp_ms() -> Result<u64>` (`utils/mod.rs:47`) | `now()` 毫秒 + NTP 偏移，`saturating_add`；负值返回 `Other("Timestamp underflow")` |
| `get_local_timestamp_ms_i64` | `pub fn get_local_timestamp_ms_i64() -> Result<i64>` (`utils/mod.rs:65`) | 同上但 i64；u64→i64 溢出（约 2262 年）报错 |
| `generate_random_string` | `#[must_use] pub fn generate_random_string(len: usize) -> String` (`utils/mod.rs:77`) | rand 0.9 `rng()` + Alphanumeric 采样，输出 `[A-Za-z0-9]` |
| `generate_random_uuid` | `pub fn generate_random_uuid() -> Result<Uuid>` (`utils/uuid.rs:9`) | 恒为 `Ok(Uuid::new_v4())`，Result 仅为 API 稳定性 |
| `NodeGetVersion::get` | `#[must_use] pub fn get() -> &'static Self` (`utils/version.rs:50`) | 从 `env!` 宏构建 13 字段结构体，缓存于 `OnceLock`；`binary_type` 由 `for-server`/`for-agent` feature 决定（"Server"/"Agent"/"Unknown"） |

### for-server 专用工具

| 名称 | 签名 | 行为 |
|------|------|------|
| `generate_error_message` | `pub fn(error_id: impl Into<i128>, error_message: &str) -> serde_json::Value` (`utils/error_message.rs:15`) | 构建 JsonError 序列化为 Value；失败时回退到硬编码 `{"error_id":101,"error_message":"Failed to serialize error"}` |
| `error_to_raw` | `pub fn error_to_raw(code: impl Into<i128>, msg: &str) -> Result<Box<RawValue>>` (`utils/error_message.rs:33`) | JsonError→RawValue，失败转 `SerializationError` |
| `nodeget_error_to_raw` | `pub fn nodeget_error_to_raw(error: &NodegetError) -> Result<Box<RawValue>>` (`utils/error_message.rs:46`) | 经 `to_json_error` 再转 |
| `anyhow_error_to_raw` | `pub fn anyhow_error_to_raw(error: &anyhow::Error) -> Result<Box<RawValue>>` (`utils/error_message.rs:56`) | `anyhow_to_nodeget_error` → `nodeget_error_to_raw`，`rpc_exec!` 桥接任意错误 |
| `to_raw_json` | `pub fn to_raw_json<T: Serialize>(val: T) -> Result<Box<RawValue>>` (`utils/server_json.rs:16`) | 失败时 `error!` 并返回 `SerializationError` |
| `to_raw_json_with_fallback` | `pub fn<T: Serialize>(val: T) -> Result<Box<RawValue>>` (`utils/server_json.rs:27`) | 失败时改序列化 `JsonError{error_id:101,...}`，保证几乎总有 RawValue 返回 |
| `try_parse_json_field` | `pub fn(map: &mut Map, key: &str)` (`utils/server_json.rs:45`) | 若 `map[key]` 是字符串且可解析为 JSON，则替换为解析后的结构（处理 DB 字符串化 JSON 列） |
| `rename_key` | `pub fn(map, old_key, new_key)` (`utils/server_json.rs:58`) | 删旧键、按新键插入同值；旧键不存在则 no-op |
| `rename_and_fix_json` | `pub fn(map, old_key, new_key)` (`utils/server_json.rs:69`) | 组合：移除旧键、字符串尝试 JSON 解析、按新键插入 |

### RBAC 数据模型

| 名称 | 签名 | 行为 |
|------|------|------|
| `NameValidator` | `pub trait NameValidator: Sized { fn validate(name: &str) -> Result<Self, error::NodegetError>; }` (`lib.rs:19`) | 输入命名类型的 Sized + fallible 构造契约，失败应返回 `InvalidInput` |
| `Token` | `{ version: i32, token_key: String, timestamp_from: Option<i64>, timestamp_to: Option<i64>, token_limit: Arc<Vec<Limit>>, username: Option<String> }` (`data_structure.rs:14`) | snake_case serde；`token_limit` 用 Arc 包裹使认证返回低成本克隆 |
| `Limit` | `{ scopes: Vec<Scope>, permissions: Vec<Permission> }` (`data_structure.rs:32`) | 一个 Limit = (scopes 中任一) × (permissions 中任一) 的合取 |
| `Scope` | `enum { Global, AgentUuid(Uuid), KvNamespace(String), JsWorker(String), StaticBucket(String), Db(String) }` (`data_structure.rs:42`) | Derives Hash（可入 HashSet）；snake_case serde |
| `Permission` | tagged union（每个变体内嵌子枚举，`data_structure.rs:60`） | 覆盖全部业务模块（StaticMonitoring/DynamicMonitoring/Task/Crontab/...） |
| `TokenCreationRequest` | `{ username/password/timestamp_*/version: Option<...>, token_limit: Vec<Limit> }` (`create.rs:9`) | snake_case；身份字段均可选，`token_limit` 必填（可为空 Vec） |
| `TokenOrAuth` | `enum { Token(String,String), Auth(String,String) }` (`token_auth.rs:11`) | snake_case serde（变体 `"token"`/`"auth"`）；Debug+Clone+PartialEq+Eq |
| `TokenOrAuth::from_full_token` | `pub fn from_full_token(full_token: &str) -> Result<Self, String>` (`token_auth.rs:23`) | `split_once(':')` → Token（**冒号优先**），否则 `split_once('|')` → Auth，否则 Err；空半边也接受；返回 `Result<Self,String>` |
| `TokenOrAuth` 访问器 | `token_key/token_secret/username/password/is_token/is_auth` (`token_auth.rs:35`) | 各返回 Some 仅对自身变体；`is_token`/`is_auth` 为 const |

### PermissionChecker（for-server）

| 名称 | 签名 | 行为 |
|------|------|------|
| `PermissionChecker` | 对象安全 async trait（`permission_checker.rs:28`） | 3 方法均返回 `Pin<Box<dyn Future + Send + 'a>>`，借用切片生命周期 `'a` 避免每请求 Vec 分配；`check_token_limit`/`check_super_token`/`get_token`；`Send+Sync+'static` |
| `PERMISSION_CHECKER` | `static OnceLock<Arc<dyn PermissionChecker>>` (`permission_checker.rs:56`) | 全局单例 |
| `set_permission_checker` | `pub fn set_permission_checker(Arc<dyn PermissionChecker>)` (`permission_checker.rs:61`) | `OnceLock::set`；重复注册仅 `tracing::warn!(target:"permission",...)` 并忽略 |
| `get_permission_checker` | `pub fn get_permission_checker() -> Option<&'static Arc<dyn PermissionChecker>>` (`permission_checker.rs:68`) | 未初始化返回 None |
| `require_permission_checker` | `pub fn require_permission_checker() -> Result<&'static Arc<dyn PermissionChecker>>` (`permission_checker.rs:75`) | 未注入时返回 `ConfigNotFound("PermissionChecker not initialized")` |

### 监控与 JS 结果查询

| 名称 | 签名 | 行为 |
|------|------|------|
| `StaticDataQueryField` | `enum { Cpu, System, Gpu }` (`monitoring/query.rs:12`) | Copy+Hash，snake_case；静态（5min）维度 |
| `StaticDataQueryField::column_name` | const fn | Cpu→`cpu_data`、System→`system_data`、Gpu→`gpu_data` |
| `StaticDataQueryField::json_key` | const fn | Cpu→`cpu`、System→`system`、Gpu→`gpu` |
| `DynamicDataQueryField` | `enum { Cpu, Ram, Load, System, Disk, Network, Gpu }` (`monitoring/query.rs:46`) | 动态（1s）维度 |
| `DynamicDataQueryField::column_name` | const fn | 同上加 `ram_data`/`load_data`/`disk_data`/`network_data` |
| `DynamicDataQueryField::json_key` | const fn | 同上加 `ram`/`load`/`disk`/`network` |
| `JsResultQueryCondition` | `enum`（`js_result/query.rs:8`） | Id/JsWorkerId/JsWorkerName/RunType/各种时间区间/IsSuccess/IsFailure/IsRunning/Limit/Last，snake_case；条件 AND 组合 |
| `JsResultDataQuery` | `{ condition: Vec<JsResultQueryCondition> }` (`js_result/query.rs:43`) | js-result RPC 查询体 |

### 自更新（self_update，for-agent/for-server 分段）

| 名称 | 签签 | 行为 |
|------|------|------|
| `check_if_update_needed` | `pub fn(tag: &str) -> ((u32,u32,u32),(u32,u32,u32),bool)` (`self_update.rs:173`) | 返回 (current, target, should_update)；tag 解析失败 → `((0,0,0),(0,0,0),false)`；current 解析失败 → `((0,0,0),target,false)`。**should_update 语义为 target != current，包含降级** |
| `canonical_exe_path` | `pub fn canonical_exe_path() -> Option<PathBuf>` (`self_update.rs:160`) | `current_exe()` 后反复剥离尾部 `.old`（应对历次自更新重命名） |
| `replace_binary` | `pub fn replace_binary(binary: Vec<u8>) -> bool` (`self_update.rs:248`) | 当前 exe 改名 `.old` → 写入新字节；写失败回滚重命名。**成功后 `.old` 不清理；不 chmod 新文件** |
| `restart_process` | `pub fn restart_process() -> !` (`self_update.rs:281`/`303`) | diverges；Unix 委托 `restart_process_with_exec_v`；非 Unix spawn 子进程后 `exit(0)` |
| `restart_process_with_exec_v` | `pub fn restart_process_with_exec_v() -> !` (`self_update.rs:315`) | 构造 CString 参数（含 NUL 的 argv 被跳过并 warn）、`libc::execv` 原地替换；execv 返回即失败 → `exit(1)` |
| `get_url` / `get_server_url` | `pub fn get_url(tag) -> Option<String>` / `get_server_url(tag) -> Option<String>` (`self_update.rs:226`/`235`) | 查 per-arch 表构造 `https://install.nodeget.com/releases/{name}?tag={tag}`；arch 不在表返回 None |

## 关键类型与常量

- **`NodegetError`**（`error.rs:11`）：12 变体均携 `String`；thiserror + Debug + **Clone**（错误枚举中少见，因 String 载荷）；**未派生 PartialEq**。
- **`JsonError`**（`utils/mod.rs:25`）：`error_id: i128` 与 `error_code()` 返回类型一致；RPC 错误负载标准形状。
- **`NTP_OFFSET_MS`**（`utils/mod.rs:33`）：`static NTP_OFFSET_MS: AtomicI64 = AtomicI64::new(0)`，使用 `portable_atomic` 以支持 32 位平台，Relaxed 读写。
- **`NodeGetVersion`**（`utils/version.rs:10`）：13 个 String 字段；Debug/Clone/PartialEq/Eq/Serialize/Deserialize；`VERSION_CACHE: OnceLock<NodeGetVersion>`（`version.rs:43`）缓存以避免每次调用 13 次 String 分配；`Display`（`version.rs:78`）多行人类可读格式。
- **`Token.token_limit: Arc<Vec<Limit>>`**（`data_structure.rs:14`）：serde `rc` feature 开启（Cargo.toml `serde = { features = ["rc"] }`），Arc 按值序列化；Eq 为字段级（Arc deref），非指针身份。
- **`Scope` / `Permission`**：均 `#[serde(rename_all = "snake_case")]`；Scope 派生 Hash 可入 HashSet；Permission 为 tagged union。
- **`NodeGet` 枚举**（`data_structure.rs:96`）：`ListAllAgentUuid`/`GetRtPool`/`DeleteAgentUuid`/`ExecSql`；其中 `ListAllAgentUuid`、`DeleteAgentUuid` 自 0.2.13 起 `#[deprecated]`，迁移至 `MonitoringUuid::List`/`Delete`。
- **错误码魔术常量**（`error.rs:66`）：仅存在于 `error_code()` 的 match 中，无命名 const 表；101 为 Parse/Serialization/IO 共享。
- **`ARCH_NAME` / `SERVER_ARCH_NAME`**（`self_update.rs:12`/`92`）：for-server 的常量数组，分别为 agent 24 对、server 10 对 `(target_triple, release_filename)`。
- **build.rs 常量 `UNKNOWN: &str = "UNKNOWN"`**（`build.rs:4`）：git/rustc 命令失败的回退值。

## 内部机制

### Trait 注入：PermissionChecker
`PERMISSION_CHECKER: OnceLock<Arc<dyn PermissionChecker>>`（`permission_checker.rs:56`）。server 二进制在启动时调用 `set_permission_checker()` 注入一次；业务 Crate 调用 `require_permission_checker()` 取 `&'static Arc<dyn PermissionChecker>`。trait 方法为 `Pin<Box<dyn Future+Send+'a>>`，借用切片参数生命周期 `'a` 以避免每请求分配 Vec。所有真实工作由 server 的 `ServerPermissionChecker` 委托给 `ng_token`。该 trait 替代了原先 6 个分散的 auth trait（`ng_db::rpc::AuthProvider`、`ng_kv/static/js_worker/terminal` 的 `TokenPermissionChecker`、`ng_task::TaskAuthProvider`）。

### NTP offset 全局与时钟校正
`NTP_OFFSET_MS: AtomicI64`（`utils/mod.rs:33`）。agent 计算 `(server_time - local_time)` 后调 `set_ntp_offset_ms(offset)`；`get_local_timestamp_ms` 返回 `SystemTime::now()` 毫秒 `saturating_add(offset)`。这样 agent 上传的时间戳能与 server 时钟对齐而无需改写 SystemTime。

### 版本缓存
`NodeGetVersion::get()`（`version.rs:50`）从 `env!` 宏构建 13 字段结构体一次，存入 `OnceLock`（`version.rs:43`），后续调用返回 `&'static` 避免 13 次 String 分配。

### 构建脚本回退
`build.rs` 在编译期调用 git 与 `rustc -vV`。git 不可用或仓库浅克隆时，`run()` 对所有 git 字段返回 `"UNKNOWN"`（`build.rs:6-15`），**永不构建失败**。`rerun-if-changed=../../.git/HEAD` 与 `../../.git/refs/`（`build.rs:77-78`，路径相对 Crate 目录的 `../../` 即仓库根）保证新提交刷新版本。

### 自更新二进制替换与回滚
`replace_binary`（`self_update.rs:248`）将当前 exe 改名为 `.old`，把新字节写入原路径；写失败则将 `.old` 改回。`canonical_exe_path`（`self_update.rs:160`）剥离尾部 `.old` 扩展，使重复自更新不会累积 `.old.old`。`.old` 备份在成功后**故意保留**（用作回滚素材 / 清理交由运维）。

### execv 进程重启
Unix 上 `restart_process_with_exec_v`（`self_update.rs:315`）调用 `libc::execv` 原地替换进程映像（无 fork），调用 future 永不再被 poll；execv 返回即失败 → `exit(1)`。非 Unix 上 `restart_process`（`self_update.rs:281`）spawn 子进程并 `exit(0)`。含 NUL 字节的 argv 被过滤并 warn（`self_update.rs:335-343`）。SAFETY 由拥有的 CString 生命周期保证。

### 错误码分组
`NodegetError::error_code` 将 ParseError/SerializationError/IoError 归入 101（`error.rs:76`）。RPC `error_id` 因此对这三者相同——**调用者无法仅凭 error_id 区分 parse 与 IO**。

### anyhow → NodegetError 还原
`anyhow_to_nodeget_error`（`error.rs:114`）将 `anyhow::Error` downcast 为 `NodegetError`（clone）或包装为 `Other`；`anyhow_error_to_raw`（`error_message.rs:56`）链式接到 `nodeget_error_to_raw`。这是 `rpc_exec!` 把任意 handler 错误转为统一 `{error_id,error_message}` RawValue 的桥梁。

### RPC RawValue 回退
`to_raw_json_with_fallback`（`server_json.rs:27`）确保 RPC 返回值始终可序列化：真实值序列化失败则改发 `JsonError{error_id:101,...}`，仅当回退本身也失败才报错。

## Crate 内部约定

- **非标准 feature 方案**：ng-core 使用 `for-server` / `for-agent`（均映射到 `dep:libc`），而非其他业务 Crate 的 `default=[]` vs `server` 模式（见 CLAUDE.md "Exception"）。
- **for-server only 模块**：`utils::error_message`、`utils::server_json`、`permission::permission_checker`（均 `#[cfg(feature = "for-server")]`）；agent 构建不含这些。
- **统一 snake_case serde**：所有可序列化枚举/结构体用 `#[serde(rename_all = "snake_case")]`（`TokenOrAuth` 变体 `"token"`/`"auth"`，`Scope` 变体 `"global"`/`"agent_uuid"`/`"kv_namespace"`/`"js_worker"`/`"static_bucket"`/`"db"`）。
- **`Token.token_limit: Arc<Vec<Limit>>`**：serde `rc` feature 开启，Arc 按值序列化；Eq 为字段级（`PartialEq` deref），非指针身份。
- **中文注释**：Crate 内注释、文档注释、内联说明均为中文，保持一致。
- **`#[must_use]`**：施加于纯 const 访问器（`column_name`、`json_key`、`token_key`、`is_token`、`NodeGetVersion::get`）与纯构造器。
- **错误码无命名表**：仅存在于 `error_code()` 的 match；101 由 Parse/Serialization/IO 共享。
- **`Result<T> = anyhow::Result<T>`**：Crate 内返回 Result 的函数均用 anyhow，非自定义错误。
- **`TokenOrAuth` 分隔符语义是承重的**：同时含 `:` 与 `|` 时 `:`（Token）优先；空半边被接受。
- **VERGEN_* 经 `env!` 编译期消费**：因 `build.rs` 总会发射（默认 "UNKNOWN"），`env!` 必然成功。

## 注意事项与陷阱

- **`should_update` 含降级**（`crates/ng-core/src/self_update.rs:150`）：返回 `target != current` 而非 `target > current`。降级（current v0.5.2 → target v0.5.1）会返回 true 并触发替换。维护者若要禁止降级，必须自行加 `>` 判断。
- **`from_full_token` 冒号优先且接受空半边**（`crates/ng-core/src/permission/token_auth.rs:23`）：`"key:secret|extra"` 解析为 `Token("key","secret|extra")`；含 `:` 的用户名会被强制 Token 模式；`":secret"` → `Token("","secret")`。调用者必须自行校验 key/username 非空。返回 `Result<Self,String>`，**非 NodegetError**。
- **`Result<T>` 是 anyhow**（`crates/ng-core/src/error.rs:107`）：函数签名 `-> Result<X>` 接受任意 anyhow 错误，会丢失结构化变体，除非用 `anyhow_to_nodeget_error` 还原。
- **error_id 为 i128**（`crates/ng-core/src/error.rs:66`）：消费方（含 JS）须处理 128 位整数；RPC 线格式会序列化为 JSON 数字。
- **`replace_binary` 不保留文件模式**（`crates/ng-core/src/self_update.rs:248`）：`std::fs::write` 不复制原文件权限，新文件按 umask 默认值。若 umask 屏蔽 0111，Unix 上新二进制可能缺少可执行位，导致随后的 execv/restart 失败。运维应验证替换后权限。
- **`.old` 备份永不清理**（`crates/ng-core/src/self_update.rs:248`）：成功后 `<exe>.old` 永久留存（`canonical_exe_path` 会剥离，故只保留一个），但旧备份从不删除。
- **`set_permission_checker` 重复注册静默忽略**（`crates/ng-core/src/permission/permission_checker.rs:61`）：`OnceLock::set` 失败仅 warn，第一个 impl 胜出，第二个被丢弃。server 启动顺序重要，必须恰好注册一次。
- **`require_permission_checker` 未注入时返回 ConfigNotFound**（`crates/ng-core/src/permission/permission_checker.rs:75`）：在 server boot 完成前调用的业务 RPC 会得到误导性的 "Config not found"，而非认证错误。
- **`NodeGet::ListAllAgentUuid` / `DeleteAgentUuid` 已 deprecated**（`crates/ng-core/src/permission/data_structure.rs:96`）：新代码必须用 `MonitoringUuid::List`/`Delete`。旧变体仅与自身比较相等——携带旧变体的 token **不会**满足对新 `MonitoringUuid` 变体的检查，可能需要迁移已存储的 token。
- **两个不同的 ExecSql 权限**（`crates/ng-core/src/permission/data_structure.rs:60`）：`Permission::NodeGet(NodeGet::ExecSql)` 是对主库的全信任任意 SQL（文档化的安全风险，SQLite 上 `ATTACH DATABASE` 提权）；`Permission::Db(Db::ExecSql)` 是 `db_registry` 作用域的 SQL exec。授权/检查代码必须选对变体，二者不可互换。
- **`get_local_timestamp_ms` 下溢报错**（`crates/ng-core/src/utils/mod.rs:47`）：NTP 校正后毫秒为负则返回 `Other("Timestamp underflow")`。Relaxed 读写对 i64 无 torn read，但跨线程偏移可见性不同步于其他内存——可作时钟偏移提示，**切勿当同步原语使用**。
- **`VERGEN_CARGO_TARGET_TRIPLE` 来自 `$TARGET` 环境变量**（`crates/ng-core/build.rs:34`）：非 cfg!/target_ 检测。若 `$TARGET` 未设置（如 rust-analyzer），变为 "UNKNOWN"，`build_release_url`（`self_update.rs:205`）返回 None——自更新对该构建静默无 URL。git 缺失时所有 git 字段同样回退 UNKNOWN。
- **`NodeGetVersion::get` 用 `env!` 非 `option_env!`**（`crates/ng-core/src/utils/version.rs:50`）：因 `build.rs` 总会发射，当前可编译；若有人禁用 build.rs，`env!` 会在编译期失败。`build.rs` 与 `version.rs` 的名字必须保持同步。
- **`restart_process_with_exec_v` 丢弃含 NUL 的 argv**（`crates/ng-core/src/self_update.rs:335`）：含嵌入 NUL 的参数被过滤（仅 warn）。重启后进程的 argv 会与原进程不同——自更新后的行为变化。
- **`Token` 的 Arc Eq 语义**（`crates/ng-core/src/permission/data_structure.rs:24`）：serde `rc` feature 下，相等性为字段级（Arc deref），两个结构相同但 Arc 不共享的 token 比较相等。这是有意语义，但会令期望 Arc 身份的读者意外。

## 依赖关系

ng-core 是工作区最底层共享 Crate，被 server 与 agent 二进制以及几乎全部业务 Crate（ng-db、ng-infra、ng-config、ng-monitoring、ng-token、ng-kv、ng-task、ng-crontab、ng-js-runtime、ng-js-worker、ng-static、ng-terminal）直接或间接依赖。主要外部依赖：`serde`/`serde_json`（含 `rc` feature）、`anyhow`、`thiserror`、`rand`（0.9）、`uuid`、`portable_atomic`、`tracing`、以及 `libc`（经 `for-server`/`for-agent` feature）。Agent 仅启用 `for-agent`（不引入 `error_message`/`server_json`/`permission_checker` 等 server-only 模块），server 启用 `for-server`。
