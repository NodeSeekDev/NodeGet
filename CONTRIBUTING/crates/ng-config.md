# ng-config — 配置 schema、CLI 与全局单例

> 概览：ng-config 集中管理 NodeGet 的配置与命令行入口。它定义了 `AgentConfig` 与 `ServerConfig` 的 TOML schema（含 `auto_gen` UUID 自举机制）、两个二进制共用的 palc CLI 参数，以及三个全局 `OnceLock` 单例——分别持有运行期 `ServerConfig`、其文件路径、以及用于热重载信号的 `tokio::sync::Notify`。在 `server` feature 下，额外暴露 `read_config` / `edit_config` 两个自由函数，由 server 二进制挂载到 `nodeget-server` RPC 命名空间；二者均强制 Super Token 鉴权，`edit_config` 执行原子写 + 热重载通知。

## 模块结构

```
crates/ng-config/src/
├── lib.rs                 # 模块装配；三个 OnceLock 单例及其访问/设置函数
├── args_parse/
│   ├── mod.rs             # 纯模块声明：pub mod agent; pub mod server;
│   ├── server.rs          # ServerArgs / ServerCommand（palc Parser + Subcommand）
│   └── agent.rs           # AgentArgs（扁平结构，无子命令）
├── config/
│   ├── mod.rs             # config 模块根 + 共享 UUID auto_gen 工具（serde 反序列化器 + 文本改写）
│   ├── server.rs          # ServerConfig 及嵌套结构 + get_and_parse_config 加载器
│   └── agent.rs           # AgentConfig、Server（per-connection）、IpProvider、默认常量与 *_or_default 辅助
└── server_rpc.rs          # read_config / edit_config 自由异步函数（server feature 门控）
```

模块可见性：`pub mod config;`、`pub mod args_parse;` 始终可用；`pub mod server_rpc;` 由 `#[cfg(feature = "server")]` 门控（`crates/ng-config/src/lib.rs:21`、`:26-27`）。

## 公共 API

### 全局单例访问与设置（`lib.rs`）

| 函数 | 签名 | 行为 |
|------|------|------|
| `get_server_config` | `pub fn get_server_config() -> Option<&'static RwLock<ServerConfig>>` (`crates/ng-config/src/lib.rs:45`) | 返回全局配置单例；启动期 `set_server_config` 之前为 `None`。调用方需对返回的 `RwLock` 取 `read()` 锁。`#[must_use]` |
| `get_server_config_path` | `pub fn get_server_config_path() -> Option<&'static str>` (`crates/ng-config/src/lib.rs:51`) | 返回配置文件路径（借用自静态 `String`），未设置时为 `None`。`#[must_use]` |
| `get_reload_notify` | `pub fn get_reload_notify() -> Option<&'static tokio::sync::Notify>` (`crates/ng-config/src/lib.rs:57`) | 返回热重载 `Notify`，server 二进制订阅 `.notified()` 以响应 `edit_config`。`#[must_use]` |
| `set_server_config` | `pub fn set_server_config(config: ServerConfig) -> anyhow::Result<()>` (`crates/ng-config/src/lib.rs:70`) | 已初始化时取写锁覆盖 `*guard = config`；未初始化时 `OnceLock::set`，并发竞态失败返回 `Err("Failed to set SERVER_CONFIG")`。幂等 |
| `set_server_config_path` | `pub fn set_server_config_path(path: String) -> Result<(), String>` (`crates/ng-config/src/lib.rs:90`) | 一次性设置；第二次调用返回 `Err("SERVER_CONFIG_PATH already set")`。错误类型为 `String`（非 anyhow） |
| `init_reload_notify` | `pub fn init_reload_notify()` (`crates/ng-config/src/lib.rs:97`) | 经 `get_or_init` 创建 `Notify`，幂等 |

### Server RPC 自由函数（`server_rpc.rs`，`server` feature）

| 函数 | 签名 | 行为 |
|------|------|------|
| `read_config` | `pub async fn read_config(token: String) -> jsonrpsee::core::RpcResult<String>` (`crates/ng-config/src/server_rpc.rs:101`) | Super Token 鉴权 → 校验路径位于 CWD 内且为普通文件 → 返回原始 TOML 文本（非解析后的类型化配置） |
| `edit_config` | `pub async fn edit_config(token: String, config_string: String) -> jsonrpsee::core::RpcResult<bool>` (`crates/ng-config/src/server_rpc.rs:155`) | Super Token 鉴权 → 用 `toml::from_str::<ServerConfig>` 预校验（丢弃值）→ 路径校验 → 原子写 `{path}.tmp.{uuid_v4}` 再 `rename` 覆盖原文件（失败则删除临时文件）→ `RELOAD_NOTIFY.notify_one()`（若已初始化）→ 返回 `true` |

### 配置加载器

| 函数 | 签名 | 行为 |
|------|------|------|
| `ServerConfig::get_and_parse_config` | `pub async fn get_and_parse_config(path: impl AsRef<Path>) -> Result<ServerConfig, Box<dyn Error + Send + Sync>>` (`crates/ng-config/src/config/server.rs:140`) | 读文件 → 预解析 `toml::Value` → 若 `server_uuid` 大小写不敏感等于 `"auto_gen"` 则生成 `Uuid::new_v4()`、调用 `replace_auto_gen_uuid`、原子写（`.tmp` + `rename`）→ 解析新内容。**不**调用 `set_server_config`；**不**做 TOML 之外的语义校验 |
| `AgentConfig::get_and_parse_config` | `pub async fn get_and_parse_config(path) -> Result<AgentConfig, Box<dyn Error+Send+Sync>>` (`crates/ng-config/src/config/agent.rs:238`) | 同样的 auto_gen 自举，之后执行 3 项校验（见下） |

### CLI 参数

| 类型 | 签名 | 行为 |
|------|------|------|
| `ServerArgs` / `ServerCommand` | `pub struct ServerArgs { command: ServerCommand }`；`impl { pub fn par() -> Self; pub const fn config_path(&self) -> &str }` (`crates/ng-config/src/args_parse/server.rs:13`、`:20`、`:58`、`:79`) | palc（Parser + Subcommand）。`ServerCommand` 变体：`Serve{config}`、`Init{config}`、`RollSuperToken{config}`、`GetUuid{config}`、`Version`。`par()` 在零参数时自动打印帮助并 `exit(0)`。`config_path()` 返回相关路径，`Version` 返回 `""` |
| `AgentArgs` | `pub struct AgentArgs { config, version, dry_run }`；`impl { pub fn par() -> Self }` (`crates/ng-config/src/args_parse/agent.rs:14`、`:36`) | 扁平结构。默认值：`config = "config.toml"`（`default_value_t = DEFAULT_AGENT_CONFIG_PATH`）、`version = false`、`dry_run = false` |

### 配置 DTO

| 类型 | 行为 |
|------|------|
| `ServerConfig` / `DatabaseConfig` / `LoggingConfig` / `MonitoringBufferConfig` / `AgentConfig` / `Server`(agent) / `IpProvider` | serde 配置 DTO / enum，统一支持 `Serialize` / `Deserialize` / `Clone`；多数类型同时 `derive(Debug)`，例外：agent 的 `Server` 仅 `#[derive(Serialize, Deserialize, Clone)]` + 手写 `Debug`（隐藏 token）；`IpProvider` 额外 `derive(Copy)`。`DatabaseConfig.database_url` 与 `ServerConfig.ws_listener` 为必填，其余皆 `Option<T>`。`IpProvider` 序列化为小写（`"ipinfo"` / `"cloudflare"`） |

## 关键类型与常量

### lib.rs 中的全局单例

| 名称 | 类型 | 行 |
|------|------|----|
| `SERVER_CONFIG` | `OnceLock<RwLock<ServerConfig>>` (`crates/ng-config/src/lib.rs:37`) | 唯一带内部 `RwLock` 的单例；读者取 `read()`，热重载取 `write()` 换入 |
| `SERVER_CONFIG_PATH` | `OnceLock<String>` (`crates/ng-config/src/lib.rs:38`) | 配置文件绝对路径，启动期设置一次，`server_rpc` 据此定位文件 |
| `RELOAD_NOTIFY` | `OnceLock<tokio::sync::Notify>` (`crates/ng-config/src/lib.rs:39`) | 热重载信号；`init_reload_notify()` 创建，`edit_config` 调 `notify_one()` 唤醒 server 监听任务 |

### ServerConfig 及嵌套（`config/server.rs`）

- `ServerConfig` (`crates/ng-config/src/config/server.rs:13`)：`server_uuid: uuid::Uuid`（`#[serde(deserialize_with = "deserialize_uuid_or_auto")]`）、`ws_listener: String`（必填）、`jsonrpc_max_connections: Option<u32>`、`enable_unix_socket: Option<bool>`、`unix_socket_path: Option<String>`、`logging: Option<LoggingConfig>`、`database: DatabaseConfig`（必填）、`monitoring_buffer: Option<MonitoringBufferConfig>`、`max_request_body_size: Option<u32>`、`max_response_body_size: Option<u32>`、`tls_cert: Option<String>`、`tls_key: Option<String>`、`static_path: Option<String>`、`db_path: Option<String>`。
- `MonitoringBufferConfig` (`crates/ng-config/src/config/server.rs:63`)：`flush_interval_ms: Option<u64>`、`max_batch_size: Option<usize>`、`channel_capacity: Option<usize>`。文档默认值（在别处应用）：flush `500ms`、batch `1000`、channel `10000`。
- `LoggingConfig` (`crates/ng-config/src/config/server.rs:87`)：`log_filter`、`json_log_file`、`json_log_filter`、`memory_log_capacity`、`memory_log_filter`，皆 `Option`。过滤语法遵循 `RUST_LOG`；虚拟 target `"db"` 在同级别展开为 `sea_orm`/`sea_orm_migration`/`sqlx`；`RUST_LOG` 环境变量覆盖 `log_filter`；`memory_log` 可经 `nodeget-server_log` RPC 查询。
- `DatabaseConfig` (`crates/ng-config/src/config/server.rs:107`)：`database_url: String`（必填）、`connect_timeout_ms`、`acquire_timeout_ms`、`idle_timeout_ms`、`max_lifetime_ms`（皆 `Option<u64>`）、`max_connections: Option<u32>`。

### AgentConfig 与相关（`config/agent.rs`）

- 默认常量 (`crates/ng-config/src/config/agent.rs:16`-`:30`)：`DEFAULT_AGENT_CONFIG_PATH = "config.toml"` (`:16`)；`DEFAULT_DYNAMIC_REPORT_INTERVAL_MS = 1000` (`:18`)；`DEFAULT_DYNAMIC_SUMMARY_REPORT_INTERVAL_MS = 1000` (`:20`)；`DEFAULT_STATIC_REPORT_INTERVAL_MS = 300_000` (`:22`)；`DEFAULT_CONNECT_TIMEOUT_MS = 1000` (`:24`)；`DEFAULT_EXEC_MAX_CHARACTER = 10_000` (`:26`)；`DEFAULT_IP_PROVIDER = IpProvider::Cloudflare` (`:28`)；`DEFAULT_NTP_SERVER = "pool.ntp.org"` (`:30`)。
- `AgentConfig` (`crates/ng-config/src/config/agent.rs:33`)：除 `agent_uuid: uuid::Uuid`（`#[serde(deserialize_with="deserialize_uuid_or_auto")]`）外全部 `Option<T>`。字段：`log_level`、`dynamic_report_interval_ms`、`dynamic_summary_report_interval_ms`、`static_report_interval_ms`、`agent_uuid`、`connect_timeout_ms`、`exec_max_character: Option<usize>`、`terminal_shell`、`ip_provider: Option<IpProvider>`、`ntp_server`、`dynamic_summary_select_disk: Option<Vec<String>>`、`dynamic_summary_select_network_interface: Option<Vec<String>>`、`server: Option<Vec<Server>>`。
- `Server`（per-connection）(`crates/ng-config/src/config/agent.rs:77`)：`#[derive(Serialize, Deserialize, Clone)]`（手写 `Debug`）。必填 `name: String`、`server_uuid: String`、`token: String`、`ws_url: String`；其后是一组 `Option<bool>` 开关：`allow_task`/`allow_icmp_ping`/`allow_tcp_ping`/`allow_http_ping`/`allow_web_shell`/`allow_read_config`/`allow_edit_config`/`allow_execute`/`allow_http_request`/`allow_ip`/`allow_dns`/`allow_version`/`allow_self_update`/`ignore_cert`；以及 `allow_task_type: Option<Vec<String>>`（白名单模式：在 `allow_task = true` 已启用任务订阅/处理后，优先覆盖各个具体任务的 `allow_*` 开关；取值为 `task_name()` 字符串，如 `"ping"`/`"tcp_ping"`/`"dns"`/`"execute"`）。
- `impl std::fmt::Debug for Server` (`crates/ng-config/src/config/agent.rs:126`)：显式罗列除 `token` 外的全部字段，`token` 固定输出 `"***REDACTED***"`。
- `IpProvider` (`crates/ng-config/src/config/agent.rs:154`)：`#[serde(rename_all = "lowercase")] pub enum IpProvider { IpInfo, Cloudflare }`，从 `"ipinfo"` 或 `"cloudflare"` 反序列化；手写 `Default` 返回 `DEFAULT_IP_PROVIDER`（Cloudflare），并通过 derive 获得 `Copy`。
- `*_or_default` 辅助 (`crates/ng-config/src/config/agent.rs:170`-`:215`)：`dynamic_report_interval_ms_or_default` (`:172`)、`dynamic_summary_report_interval_ms_or_default` (`:179`)、`static_report_interval_ms_or_default` (`:186`)、`connect_timeout_duration() -> Duration` (`:193`)、`exec_max_character_or_default` (`:202`)、`ip_provider_or_default` (`:209`)、`ntp_server_or_default() -> &str` (`:215`)，全部 `#[must_use]`。

### auto_gen UUID 工具（`config/mod.rs`）

- `deserialize_uuid_or_auto` (`crates/ng-config/src/config/mod.rs:18`)：`pub fn deserialize_uuid_or_auto<'de, D>(deserializer: D) -> Result<Uuid, D::Error> where D: Deserializer<'de>`。先按 `String` 反序列化；若大小写不敏感（`eq_ignore_ascii_case`）等于 `"auto_gen"` 则返回 `Err(custom)`——`auto_gen` 不允许经 serde 往返，必须先被 `get_and_parse_config` 替换；否则 `Uuid::parse_str`。
- `replace_auto_gen_uuid` (`crates/ng-config/src/config/mod.rs:42`)：`pub(crate) fn replace_auto_gen_uuid(content: &str, key: &str, uuid: &str) -> String`。逐行 TOML 文本改写器：跳过 `#` 注释与空行；对每行定位 key 边界（至 `=` 或空白前），要求 `key_end == key.len()` 且 key 比较大小写不敏感；在原始行中定位 `=`，要求值以 ASCII 引号（`'` 或 `"`）起始，检查后续 8 字符（`.get(..8)` 避免非 ASCII 边界 panic）大小写不敏感等于 `"auto_gen"`，然后重构 `before + quote + new_uuid + after_value`。其余内容原样保留；每行追加 `'\n'`。

## 内部机制

### Config 热重载生命周期

`server_rpc::edit_config` 写入 `{config_path}.tmp.{uuid_v4}`，再 `tokio::fs::rename` 覆盖目标。随机 UUID 后缀防止两次并发 `edit_config` 互相踩踏临时文件。`rename` 失败时同步 `await` 删除临时文件后返回错误。成功后 `get_reload_notify().notify_one()` 唤醒 server 的配置监听任务重新读文件——server 二进制才拥有真正的重载逻辑，ng-config 仅负责信号。

### 全局单例并发

`SERVER_CONFIG` (`crates/ng-config/src/lib.rs:37`) 是唯一带 `RwLock` 的单例：读者取 `read()` 守卫，`set_server_config`（启动期与每次重载由 server 调用）取 `write()` 并交换 `*guard`。`SERVER_CONFIG_PATH` 与 `RELOAD_NOTIFY` 为普通 `OnceLock`（无内部锁，分别 set-once / `get_or_init`）。

### auto_gen UUID 自举

`ServerConfig::get_and_parse_config` (`crates/ng-config/src/config/server.rs:140`) 与 `AgentConfig::get_and_parse_config` (`crates/ng-config/src/config/agent.rs:238`) 实现同一模式：通过 `toml::Value` 预解析检测 `uuid == "auto_gen"`（大小写不敏感）→ 生成 `Uuid::new_v4()` → 调 `replace_auto_gen_uuid` 重写文本 → 经 `.tmp`（`with_extension("tmp")`）+ `rename` 原子写 → 解析重写后文本。这使 UUID 被持久化，后续启动跳过重新生成。`deserialize_uuid_or_auto` 是兜底执行者：任何残留到解析步骤的 `auto_gen` 都被拒绝，保证重写后的解析不会静默接受未重写的 `auto_gen`。

### 配置校验

`AgentConfig::get_and_parse_config` 在解析后执行 3 项校验（`crates/ng-config/src/config/agent.rs:268`-`:300`）：(1) `connect_timeout_ms != Some(0)`；(2) 经 `HashSet`（容量预分配）校验 `server[].name` 唯一性；(3) `dynamic_report_interval_ms`（或默认值）必须是 `dynamic_summary_report_interval_ms`（或默认值）的整数倍，使用 `u64::is_multiple_of`（无符号，前置守卫 summary != 0 以防 `is_multiple_of(0,0)==true` 漏过）。`ServerConfig::get_and_parse_config` 除 TOML 格式正确性外**不**做语义校验。

### RPC 鉴权委托

`ensure_super_token` (`crates/ng-config/src/server_rpc.rs:21`) 不再使用函数指针注入，而是调用 `ng_core::permission::permission_checker::get_permission_checker()`（CLAUDE.md 所述的 OnceLock trait 注入），对解析出的 `PermissionChecker` 调 `check_super_token`。流程：`TokenOrAuth::from_full_token(token)` 失败 → `NodegetError::ParseError`；`get_permission_checker()` 未设 → `NodegetError::Other`；`check_super_token` 出错 → `NodegetError::PermissionDenied`；`is_super == false` → `Err(PermissionDenied "Super token required")`。日志 target 为 `"server"`（入口 `trace!`，拒绝时 `warn!`）。`#[rpc]` 方法注册发生在 server 二进制中——ng-config 以自由异步函数返回 `RpcResult`，由 server 挂到 `nodeget-server` 命名空间。

### RPC 错误转换

进程逻辑封装在返回 `anyhow::Result` 的异步块中；外层 `fn` `await` 后经 `ng_core::error::anyhow_to_nodeget_error(&e)` 映射为 `NodegetError`，再包装为 `jsonrpsee::types::ErrorObjectOwned`（`code = nodeget_err.error_code() as i32`、`message = format!("{nodeget_err}")`）。这是别处 `rpc_exec!` 宏的手写等价物——这两个函数返回原生类型的 `RpcResult<String>` / `RpcResult<bool>`，说明由 server 的 `#[rpc]` 包装器调用而非在此处包装。

## RPC 方法

| 命名空间 | 方法 | 参数 | 所需权限 | 行为 |
|----------|------|------|----------|------|
| `nodeget-server` | `read_config` | `token: String`（Super 凭证；`TokenOrAuth` 接受 `key:secret` 或 `username\|password`） | Super Token（id=1）— `ensure_super_token` 对所有非 super 凭证返回 `PermissionDenied` | 经 `ensure_super_token` 鉴权 → 取 `SERVER_CONFIG_PATH`（未设返回 `Other`）→ `validate_config_path` → `tokio::fs::read_to_string`。返回**原始 TOML 文本**，非类型化配置。由 server 二进制注册到 `nodeget-server` 命名空间 |
| `nodeget-server` | `edit_config` | `token: String`（Super 凭证；`TokenOrAuth` 接受 `key:secret` 或 `username\|password`）、`config_string: String`（新 TOML） | Super Token（id=1） | 鉴权 → `toml::from_str::<ServerConfig>` 预校验（落盘前拦截畸形配置，丢弃值）→ 校验路径 → 原子写 `{path}.tmp.{uuid}` 再 `rename`（失败删临时文件）→ 若 `RELOAD_NOTIFY` 已初始化则 `notify_one()`。**不**更新内存中的 `SERVER_CONFIG`——由 server 的重载监听任务重新读取并调 `set_server_config` |

鉴权流程（两方法共享）：`ensure_super_token` (`crates/ng-config/src/server_rpc.rs:21`) 按"RPC 鉴权委托"中描述的 pipeline 执行，依赖 `ng-core` 的 `PermissionChecker` 注入。

## Crate 内部约定

- **严格 clippy**：`#![warn(all, pedantic, nursery)]`，全局 allow cast lints（`cast_sign_loss`、`cast_precision_loss`、`cast_possible_truncation`）、`similar_names`、`doc_markdown`（`crates/ng-config/src/lib.rs:1-8`）。
- **中文文档注释**：rustdoc 与行内注释统一中文，符合 NodeGet 全局约定。
- **Default-None Option 模式**：所有配置旋钮为 `Option<T>`，使仅含必填字段的 TOML 即可解析；消费者调 `*_or_default()` 辅助物化有效值（`crates/ng-config/src/config/agent.rs:170-217`）。
- **Feature 门控**：`default = []` 仅暴露类型/解析器（agent 安全依赖）；`server` feature 门控 `server_rpc`（RPC + 鉴权副作用）（`crates/ng-config/src/lib.rs:26-27`）。
- **serde 派生**：多数配置结构体 `#[derive(Serialize, Deserialize, Debug, Clone)]` 以支持 TOML+JSON 双向；例外是 agent 侧 `Server` 仅 `#[derive(Serialize, Deserialize, Clone)]` + 手写 `Debug`；`IpProvider` 用 `#[serde(rename_all = "lowercase")]` 并通过 derive 获得 `Copy`（`crates/ng-config/src/config/agent.rs:75-77`、`:125-160`）。
- **自定义反序列化器**：`deserialize_uuid_or_auto` 用 `eq_ignore_ascii_case` 做大小写不敏感的 `"auto_gen"` 拒绝（`crates/ng-config/src/config/mod.rs:24`）。
- **CLI 框架**：palc（`Parser` + `Subcommand`）；`ServerArgs` 用子命令枚举，`AgentArgs` 用扁平字段 + `default_value_t`（`crates/ng-config/src/args_parse/server.rs:3`、`crates/ng-config/src/args_parse/agent.rs:4`）。
- **原子写惯用法**：`server_rpc::edit_config` 与两个 `get_and_parse_config` 一致采用"写临时文件再 rename"。**注意两者临时文件命名不同**：server RPC 加随机 UUID 后缀（`{path}.tmp.{uuid}`），加载器用 `with_extension("tmp")`。
- **日志 target**：`server_rpc.rs` 使用 `"server"`；`config/agent.rs` 在配置校验失败时用 `"config"` 发出 `warn!`。本 crate 内还会在 `args_parse/*.rs` 无 target 地输出帮助信息（`tracing::info!`）。
- **`#[must_use]`**：三个单例 getter 标注（`crates/ng-config/src/lib.rs:44`、`:50`、`:56`）；setter / init 函数未标注。
- **错误转换**：`server_rpc` 将 `anyhow::Error` → `NodegetError` → `jsonrpsee::ErrorObjectOwned`；配置解析返回 `Box<dyn Error + Send + Sync>` 以便跨 crate 异步使用。

## 注意事项与陷阱

- **`validate_config_path` 不是可靠的路径监狱**（`crates/ng-config/src/server_rpc.rs:60-69`）：以未规范化的 `std::env::current_dir()` 作允许基，却对目标路径做 canonicalize，再用 `canonical_path.starts_with(&current_dir)` 判定。因 `current_dir` 未规范化，符号链接或相对 CWD 可能造成误拒或失效约束。**维护者加固路径穿越时必须对两路径都 canonicalize 后再 `starts_with` 比较**，切勿假设此函数是健全的 jail。
- **`validate_config_path` 返回原始 `&Path`**（`crates/ng-config/src/server_rpc.rs:84`）：返回 `Path::new(config_path)`（非规范化路径），`read_config`/`edit_config` 据此读写；而 `is_file` 校验跑在规范化路径上——若该路径是指向会变化的符号链接，存在 TOCTOU 间隙。保留原始文件名是有意为之。
- **`replace_auto_gen_uuid` 是文本级改写器**（`crates/ng-config/src/config/mod.rs:42`）：仅在值的首 8 字符大小写不敏感等于 `"auto_gen"` 时替换。若引号内有前导空白、key 缩进超过值、或 TOML 使用多行/数组形式，替换会被静默跳过，文件残留字面量 `auto_gen`，随后 `toml::from_str` 解析失败（被 `deserialize_uuid_or_auto` 拒绝）。此外每行追加 `'\n'`，原本不以换行结尾的文件会多出一行（轻微内容漂移）。
- **临时文件命名不一致**（`crates/ng-config/src/config/server.rs:158`）：`get_and_parse_config` 用 `with_extension("tmp")`——这是替换而非追加扩展名：`config.toml` → `config.tmp`（正常）、`config`（无扩展名）→ `config.tmp`、`config.beta.toml` → `config.beta.tmp`。而 `server_rpc::edit_config` 用 `{path}.tmp.{uuid}`，两个写者若同时运行可能在 `.tmp` 名上碰撞，互不协调。
- **多倍数校验仅在加载时强制**（`crates/ng-config/src/config/agent.rs:286-300`）：使用 `*_or_default()` 比较，故两者皆缺省时平凡通过（1000 是 1000 的倍数）。不变式：`dynamic_report_interval_ms` 必须是 `dynamic_summary_report_interval_ms` 的整数倍——代码仅在加载时校验，后续修改不重新校验。`summary==0` 守卫必须保留，否则 `is_multiple_of(0,0)==true` 会漏过。
- **`par()` 控制流易误读**（`crates/ng-config/src/args_parse/server.rs:64-67`）：当前无参数时之所以退出，是因为构造的 `-h` 解析在 palc 中走 `Err` 分支，随后记录帮助并 `exit(0)`；代码本身并没有对 `try_parse_from(... )` 的 `Ok` 路径显式退出，若未来 help 行为变化，这段控制流会跟着变。两函数均含 `// todo: add check`，表示解析后校验被有意推迟，**切勿假设参数已校验**。
- **`Server` 的 `Debug` redact 是显式白名单**（`crates/ng-config/src/config/agent.rs:126`）：手写 `Debug` 将 `token` 输出为 `"***REDACTED***"`，但逐字段罗列——新增敏感字段若不更新此 impl **不会**被 redact。且派生的 `Clone`/`Serialize` 仍泄露 token，仅 `Debug` 安全。
- **两个 setter 语义不对称**（`crates/ng-config/src/lib.rs:70-83` 对比 `:90-94`）：`set_server_config` 返回 `anyhow::Result<()>` 且容忍重复调用（幂等）；`set_server_config_path` 返回 `Result<(), String>` 并在第二次调用报 `"SERVER_CONFIG_PATH already set"`。维护者切勿假设两者对称。
- **`edit_config` 不保证热应用**（`crates/ng-config/src/server_rpc.rs:163`）：预解析 `ServerConfig` 校验 TOML 但丢弃结果，从不调 `set_server_config`。内存配置更新发生在 server 二进制的重载监听任务（由 `RELOAD_NOTIFY` 触发）。若重载监听缺失或故障，`edit_config` 返回 `Ok(true)` 但运行中的 server 仍用旧配置——RPC 成功不等于已生效。

## 依赖关系

ng-config 依赖 `ng-core`（`PermissionChecker` 注入、`NodegetError` 与 `anyhow_to_nodeget_error`、`TokenOrAuth`），以及外部 `serde`、`tokio`、`toml`、`uuid`、`palc`、`jsonrpsee`（仅 `server` feature）、`tracing`、`anyhow`。它在工作区中处于上游：`server` 二进制（启用 `server` feature，挂载 `read_config`/`edit_config` 到 RPC 并消费全局单例与 CLI）、`agent` 二进制（用 `default` feature 的 `AgentConfig` 与 `AgentArgs`），以及任何需要读写 `ServerConfig`/`AgentConfig` 的 crate 都依赖它。agent 端依赖不启用 `server` feature，故不会被 `server_rpc` 的 RPC/鉴权代码污染。
