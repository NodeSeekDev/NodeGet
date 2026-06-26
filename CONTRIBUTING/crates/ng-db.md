# ng-db — 数据库层（实体、主库连接、用户库注册表、DB 相关 RPC）

> 概览：ng-db 是 NodeGet 的数据库层。它持有全部 13 张表的 SeaORM 实体定义、全局主库连接单例（`get_db`/`set_db`）、主库连接初始化（含迁移与 SQLite PRAGMA），以及管理用户自建 SQLite 连接池的 `DbRegistryManager`（对应 `db` 命名空间的 create/read/update/delete/list/exec_sql RPC 与生命周期/清理）。同时暴露 `nodeget-server` 命名空间下针对主库的存储核算、SQL 执行与后端类型查询 RPC。Agent 以默认 feature 依赖本 crate（仅实体类型）；Server 开启 `server` feature 后追加连接初始化、注册表管理器与全部 RPC handler。

## 模块结构

```
crates/ng-db/src/
├── lib.rs                       # Crate 根：主库 OnceLock 单例、entity 再导出、server 模块条件编译
├── entity/                      # 13 个 SeaORM 实体（始终编译，无 feature 门控）
│   ├── mod.rs                   # 声明 13 个子模块并再导出 prelude
│   ├── prelude.rs               # 为每张表再导出 Entity 别名（如 prelude::Crontab）
│   ├── crontab.rs               # cron 任务定义
│   ├── crontab_result.rs        # cron 执行历史
│   ├── db_registry.rs           # 用户库元数据
│   ├── dynamic_monitoring.rs    # 每秒动态监控指标
│   ├── dynamic_monitoring_summary.rs  # 预聚合动态指标
│   ├── js_result.rs             # JS worker 执行历史
│   ├── js_worker.rs             # 注册的 QuickJS 脚本及资源限制
│   ├── kv.rs                    # KV 存储（命名空间 + JSON 值）
│   ├── monitoring_uuid.rs       # Agent UUID 注册（软删除）
│   ├── static_file.rs           # 静态桶配置
│   ├── static_monitoring.rs     # 5 分钟静态采样（含 data_hash 去重）
│   ├── task.rs                  # 派发任务执行日志
│   └── token.rs                 # 鉴权 Token（哈希密钥 + 用户名|密码 + JSON Limit）
├── db_connection.rs   (server)  # 主库连接初始化 + 迁移 + PRAGMA
├── db_registry.rs     (server)  # 用户库连接池管理器 + JSON 转换辅助
└── rpc/               (server)
    ├── mod.rs                   # RPC 基础设施：token_identity、rpc_exec!、RpcHelper、to_rpc_error
    ├── db/
    │   ├── mod.rs               # db 命名空间 #[rpc] trait + DbRpcImpl + rpc_module()
    │   ├── auth.rs              # check_db_permission + validate_db_name（含单测）
    │   ├── create.rs            # db.create
    │   ├── read.rs              # db.read
    │   ├── update.rs            # db.update（三阶段重命名）
    │   ├── delete.rs            # db.delete
    │   ├── list.rs              # db.list
    │   └── exec_sql.rs          # db.exec_sql（用户库）
    └── nodeget/
        ├── mod.rs               # nodeget-server 子模块声明
        ├── database_storage.rs  # 主库逐表字节核算（super token）
        ├── exec_sql.rs          # 主库全信任 SQL（NodeGet::ExecSql）
        └── get_database_type.rs # 主库后端类型字符串

crates/ng-db/migration/src/
├── lib.rs                       # Migrator：19 步迁移按时间序列举
└── main.rs                      # CLI 入口（已被注释为 no-op）
```

## 公共 API

| 名称 | 签名 | 行为 |
|---|---|---|
| `get_db` | `pub fn get_db() -> Option<&'static sea_orm::DatabaseConnection>` (`lib.rs:43`) | 仅在 `set_db` 被 `init_db_connection` 调用后返回 `Some(&'static ...)`；此前为 `None`。Server 模块共享该唯一连接。 |
| `set_db` | `pub fn set_db(conn: sea_orm::DatabaseConnection)` (`lib.rs:52`, server) | 用 `ManuallyDrop` 包裹后存入 `OnceLock`。重复调用会记录 `warn`（target `db`）并丢弃新连接。 |
| `take_and_close_db` | `pub unsafe fn take_and_close_db()` (`lib.rs:66`, `#[allow(dead_code)]`) | 通过 `(&raw const DB).cast_mut()` + `(*ptr).take()` 回收 `ManuallyDrop` 并正确析构；调用后 `get_db()` 返回 `None`。当前无任何调用方，serve.rs 默认不调用。 |
| `entity::*` | `pub mod entity` (`lib.rs:27`) | 13 个实体模块始终可用（无 feature 门控），每个导出 `Model`/`Entity`/`ActiveModel`/`Column`/`PrimaryKey`/`Relation`/`ActiveModelBehavior`。 |
| `init_db_connection` | `pub async fn init_db_connection(config: DbConnectionConfig) -> anyhow::Result<()>` (`db_connection.rs:60`, server) | 配置 `ConnectOptions`（`sqlx_logging_level = Trace`），建连后**先**置 PRAGMA `auto_vacuum=INCREMENTAL` **再** `Migrator::up`，随后对 SQLite 设置 WAL/synchronous=NORMAL/busy_timeout=5000/foreign_keys=ON/cache_size=-64000，回读 auto_vacuum 用于诊断，最后 `set_db`。 |
| `DbConnectionConfig` | `pub struct { database_url, connect_timeout_ms, acquire_timeout_ms, idle_timeout_ms, max_lifetime_ms, max_connections }` (`db_connection.rs:17`, server) | 所有超时以毫秒计；`Default`：`""/3000/3000/60000/1_800_000`，`max_connections=10`。 |
| `DbRegistryManager` | 见下方"内部机制" | 全局单例（`OnceLock<Arc<...>>`）；`init` 幂等（`Once`），启动 seed+清理任务；`global()` 在 init 前返回 `None`。 |
| `DbRegistryManager::has_conn` | `pub fn has_conn(&self, name: &str) -> bool` (`db_registry.rs:265`) | 读锁 `pools.contains_key(name)`；轻量存在性检查，避免克隆 `DatabaseConnection`。 |
| `DbRegistryManager::get_conn` | `pub fn get_conn(&self, name: &str) -> Option<DatabaseConnection>` (`db_registry.rs:277`) | 读锁，刷新 `last_used_ms = now_ms`（Relaxed），克隆连接。 |
| `DbRegistryManager::get_db_path` | `pub fn get_db_path(&self, name: &str) -> String` (`db_registry.rs:257`) | `format!("{}/{}.db", db_path.trim_end_matches('/'), name)`。 |
| `DbRegistryManager::create_conn` | `pub async fn create_conn(&self, name: &str, max_lifetime_ms: Option<i64>) -> anyhow::Result<DatabaseConnection>` (`db_registry.rs:306`) | 连接 `sqlite://...` URL、设 PRAGMA，再 upsert `db_registry`（存在则 `db_connections +=1`，否则插入新行带 `created_at=now_ms`），最后插入 `pools`。**不校验 name**。 |
| `DbRegistryManager::remove_conn` | `pub async fn remove_conn(&self, name: &str) -> anyhow::Result<()>` (`db_registry.rs:385`) | 移除 `pools` 条目、按 id 删 `db_registry` 行（失败仅 log）、删除 `{name}.db` 及 `-wal`/`-shm` 文件（失败仅 log）；始终返回 `Ok(())`。 |
| `DbRegistryManager::list_all` | `pub async fn list_all(&self) -> anyhow::Result<Vec<DbInfo>>` (`db_registry.rs:429`) | `db_registry::find` 按 name 升序，映射为 `DbInfo`（`is_active` 取自 `pools.contains_key`）。 |
| `DbRegistryManager::shutdown` | `pub async fn shutdown(&self)` (`db_registry.rs:461`) | 置 `cancelled`、`notify_one()`，`cleanup_handle.lock().unwrap().take()` 后以 5 秒超时 await，记录结果（clean exit / panic / timeout）。 |
| `row_to_json` | `pub fn row_to_json(r: &sea_orm::QueryResult) -> serde_json::Value` (`db_registry.rs:523`, server) | 按 `column_names()` 迭代，每列调 `try_column_as_json`，返回 Object。 |
| `json_to_sea_value` | `pub fn json_to_sea_value(json: &Value) -> sea_orm::Value` (`db_registry.rs:578`, server, `#[must_use]`) | Null→Json(None)，Bool→Bool，Number→BigInt/BigUnsigned/Double（不可表示则 String），String→String，Array/Object→Json(Some(Box))。 |
| `db::Rpc` / `RpcServer` | `#[rpc(server, namespace="db")] pub trait Rpc { ... }` (`rpc/db/mod.rs:55`, server) | 6 个方法，全部返回 `RpcResult<Box<RawValue>>`；由 `DbRpcImpl` 实现。 |
| `db::rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<DbRpcImpl>` (`rpc/db/mod.rs:167`) | 供 server 二进制合并进路由。 |
| `validate_db_name` | `pub fn validate_db_name(name: &str) -> anyhow::Result<()>` (`rpc/db/auth.rs:67`, server) | 非空、`len<=128`、仅 `[A-Za-z0-9_.-]`、不为 `.` 或 `..`；否则返回 `NodegetError::InvalidInput`。 |
| `token_identity` | `pub fn token_identity(token: &str) -> (&str,&str)` (`rpc/mod.rs:33`) | 按 `:` 切分得 `(key,"")`，否则按 `|` 得 `("",username)`，否则 `("???","")`。零分配借用。 |
| `rpc_exec!` | `macro` (`rpc/mod.rs:71`, `#[macro_export]`) | Ok 时 `debug!(target:"rpc", response=%TruncatedRaw(&raw))`（截断 1024 字节）；Err 时 `error!(target:"rpc", error=%e)`。 |
| `RpcHelper` | `pub trait RpcHelper` (`rpc/mod.rs:87`, server) | blanket 实现：`try_set_json<T:Serialize>()->Result<ActiveValue<Value>>`（失败 `SerializationError`）、`get_db()->Result<&'static DatabaseConnection>`（未初始化 `DatabaseError`）。 |
| `to_rpc_error` | `pub fn to_rpc_error(e: &anyhow::Error) -> ErrorObject<'static>` (`rpc/mod.rs:114`, server, `#[must_use]`) | 经 `anyhow_to_nodeget_error` 转 `NodegetError`，用 `error_code()` 作 i32 code、Display 作 message，无 data。 |

## 关键类型与常量

### lib.rs 主库单例

- `static DB: OnceLock<ManuallyDrop<DatabaseConnection>>` (`lib.rs:36`) — `ManuallyDrop` 避免进程退出时连接池的多秒 join（由 OS 回收 TCP/内存）。
- `get_db` / `set_db` / `take_and_close_db`（签名见上表）。

### DbRegistryManager 核心类型（`db_registry.rs`）

- `static MGR: OnceLock<Arc<DbRegistryManager>>` (`db_registry.rs:32`)。
- `struct TrackedConnection { conn: DatabaseConnection, last_used_ms: AtomicU64 }` (`db_registry.rs:35`) — `last_used_ms` 在 `get_conn`/`create_conn` 中以 Relaxed 序刷新。
- `pub struct DbRegistryManager { db_path: String, pools: RwLock<HashMap<String,Arc<TrackedConnection>>>, cancelled: AtomicBool, cancel_notify: Notify, cleanup_handle: Mutex<Option<JoinHandle<()>>> }` (`db_registry.rs:46`)。
- `fn now_ms_u64() -> u64` (`db_registry.rs:60`) — `SystemTime::now().duration_since(UNIX_EPOCH).expect(...).as_millis() as u64`；**系统时钟早于 1970 会 panic**。
- `pub struct DbInfo { id, name, file_path, db_connections: Option<i32>, max_lifetime_ms: Option<i64>, created_at, is_active }` (`db_registry.rs:481`)。
- `pub struct DbExecResult { success: bool, data: Vec<serde_json::Value>, row_count: u64 }` (`db_registry.rs:500`) — `exec_sql` RPC 响应。
- `fn get_main_db() -> anyhow::Result<&'static DatabaseConnection>` (`db_registry.rs:510`) — `get_db().context("Main DB not initialized")`。
- `fn try_column_as_json(r, col) -> Value` (`db_registry.rs:536`) — 探测顺序：String→i64→u32→f64→bool→`Vec<u8>`（先按 JSON 解析，失败回退 `hex::encode`）→`serde_json::Value`→Null。

### Rpc 基础设施（`rpc/mod.rs`）

- `pub struct TruncatedRaw<'a>(&'a RawValue)` (`rpc/mod.rs:47`) — Display 截断到 1024 字节（`floor_char_boundary`）并追加 `[...N bytes total]`。

### db 命名空间（`rpc/db/`）

- `pub struct DbRpcImpl` (`rpc/db/mod.rs:98`) — `impl RpcHelper for DbRpcImpl {}`；每个方法构建 target 为 `"db"` 的 `info_span!`（字段 `token_key`/`username`/`name`，exec 另带 `sql_len`），通过 `rpc_exec!` 委托给子模块函数。
- 死代码占位结构 `NameParam{name}`/`RenameParam{name,new_name}`/`ExecSqlParam{name,sql,params:Option<Value>}` (`rpc/db/mod.rs:25`) — 均 `#[derive(Deserialize)]` 且 `#[allow(dead_code)]`，**不参与实际 RPC 反序列化**。

### nodeget 命名空间（`rpc/nodeget/`）

- `const EXCLUDED_TABLES: &[&str] = &["seaql_migrations"]` (`database_storage.rs:13`)。
- `struct TableSizeRow { table_name: String, table_size: i64 }` (`database_storage.rs:17`, `FromQueryResult`)。
- `struct DatabaseStorageResponse { tables: BTreeMap<String,i64>, total: i64 }` (`database_storage.rs:26`) — tables 按名排序，total 为各项之和。
- `struct TableNameRow { table_name: String }` (`database_storage.rs:163`) — 用于 postgres 表名发现。

### 实体通用约定

所有实体 `#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]`，`Relation` enum 为空，`ActiveModelBehavior` 取默认（无 DB 端钩子）。由 sea-orm-codegen 2.0 经 `sea-orm-cli` 生成。大字段统一 `#[sea_orm(column_type = "JsonBinary")]`（存 `Json`）。

## 内部机制

### Startup/teardown 生命周期

serve.rs 调用 `init_db_connection(DbConnectionConfig)`：建连 → 在 `Migrator::up` **之前**置 `PRAGMA auto_vacuum=INCREMENTAL`（仅对全新空库生效；旧库忽略且 best-effort，错误被 `let _ =` 丢弃）→ `Migrator::up` → 设置 WAL 等 PRAGMA → 回读 auto_vacuum 用于诊断 → `set_db`。随后 serve.rs 调用 `DbRegistryManager::init(db_path)`，后者 spawn 一个任务先 `seed_from_dbreg` 再 `start_cleanup_loop`。关闭时调用 `DbRegistryManager::shutdown`（5 秒超时 join）——**默认不调用 `take_and_close_db`**（DB 为 `ManuallyDrop`，由 OS 回收）。

### 用户库连接生命周期 + 过期

`DbRegistryManager` 以 `Arc<TrackedConnection>`（`DatabaseConnection` + `AtomicU64 last_used_ms`）形式持有每个连接，外层 `RwLock<HashMap>`。`get_conn` 以 Relaxed 序刷新 `last_used_ms` 并克隆 SeaORM 连接（廉价——仅池句柄）。清理循环每 1 分钟运行（`MissedTickBehavior::Delay`），读锁下收集 `(name,last_used)`，**一次性**加载全部 `db_registry` 行避免 N+1，计算 `elapsed_ms >= lifetime_ms` 后对每项调 `remove_conn`。`max_lifetime_ms` 为 `Option`（`None` = 永不过期）。`last_used` 仅由 `get_conn`/`create_conn` 刷新，因此"过期"语义绑定"近期查询活动"而非创建时间。

### 清理循环取消

`start_cleanup_loop` 使用 `tokio::select! { biased; cancel_notify.notified() => break; ticker.tick() => cleanup_expired() }`。`shutdown` 先 `cancelled.store(true, SeqCst)` 再 `cancel_notify.notify_one()`，然后以 5 秒超时 await 句柄。biased 序保证取消始终优先于下一次 tick。

### 锁纪律

`seed_from_dbreg` 在写锁**之外**构建连接（sqlite 建连 + PRAGMA 的磁盘 I/O），仅插入时取写锁。`cleanup_expired` 读锁下收集候选，**释放锁后**再查 `db_registry` 与删除——绝不在 DB 往返期间持锁。除 `init`/`shutdown` 中对 `cleanup_handle` 使用 `.unwrap()` 外，其余锁获取均用 `unwrap_or_else(PoisonError::into_inner)` 以在他人 panic 后存活。

### db.update 三阶段重命名

`update.rs` 实现：`spawn_blocking` 重命名 `.db`（含 `-wal`/`-shm`，WAL 失败仅 warn——SQLite 下打开时自恢复）→ 更新 `db_registry` 行名 → `remove_conn(old)` + `create_conn(new, updated.max_lifetime_ms)`。若第二阶段（注册表更新）失败，`spawn_blocking` 回滚文件重命名；若第三阶段（池刷新）失败，仅 warn（DB 与文件已一致），不回滚。权限：先 `validate_db_name(new_name)`，再对 `name` 与 `new_name` **两者**各检查一次 `Db::Update`。

### exec_sql 执行模型

`db.exec_sql` 与 `nodeget::exec_sql` 都用 `query_all_raw`（统一处理 SELECT/DML/DDL/PRAGMA/CTE）。无 RETURNING 的 DML 返回 `data:[]` + `row_count:0`。两者都将结果截断到 **10_000 行**并置 `truncated:true`。params 必须是 JSON Array 或 null，每个元素经 `json_to_sea_value` 绑定。两者为**近似重复代码**（分别作用于用户池与主库），无共享 helper。

### database_storage dbstat 优化

`query_sqlite` 将 `sqlite_master`（m，含空表在内的全表清单）与 `dbstat('main',1)`（d，聚合模式——每 b-tree 一行）按 `d.name=m.name` 等值连接，取 `COALESCE(d.pgsize,0)`。等值连接让 dbstat 把 name 过滤下推到 `xBestIndex`（根页查找 + 单 b-tree 遍历），避免扫描每一页。此优化替换了在 100MB+ 库上耗时数十秒的朴素 `WHERE name IN(...)` 全扫描。依赖 `SQLITE_ENABLE_DBSTAT_VTAB`（sqlx bundled 自带）。Postgres 侧用 `unnest($1::text[])` 配合 `pg_total_relation_size` 单次往返。`EXCLUDED_TABLES = ["seaql_migrations"]` 被排除在核算之外。

### row_to_json 类型探测

见上表 `try_column_as_json`；`Vec<u8>` 先尝试作为嵌套 JSON 解析，失败则 `hex::encode`。

### auto_vacuum 预迁移陷阱

`init`/`seed`/`create_conn` 均在其它 PRAGMA 之前执行 `PRAGMA auto_vacuum=INCREMENTAL`。该语句仅在数据库**刚创建（空）**时生效；既有库保持其当前 auto_vacuum（init 时回读并记录实际值，但不会修复）。运维若要对旧库启用 INCREMENTAL 须手动转换。

## RPC 方法

| 命名空间 | 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|---|
| `db` | `create` | `token, name` | `Permission::Db(Db::Create)` on `Scope::Db(name)` | 先 `validate_db_name` 再鉴权；若 name 已存在于 `db_registry` 拒绝；经 `create_conn(name, None)` 建 SQLite 文件与注册表行。返回 `{success, data:{name, file_path}}`。 |
| `db` | `read` | `token, name` | `Permission::Db(Db::Read)` on `Scope::Db(name)` | 鉴权（**不校验 name**）→ 按 name 查 `db_registry`（缺失 `NotFound`）→ `has_conn` 报告 active。返回 `{success, data:{id,name,created_at,active}}`。 |
| `db` | `update` | `token, name, new_name` | `Db::Update` on `Scope::Db(name)` **且** `Scope::Db(new_name)` | 校验 `new_name`，对两 name 各鉴权一次；确认 old 存在、new 不存在；三阶段重命名（文件→注册表→池）。返回 `{success, data:{id,name,created_at}}`。 |
| `db` | `delete` | `token, name` | `Permission::Db(Db::Delete)` on `Scope::Db(name)` | 校验 name、鉴权、确认存在，全权委托 `remove_conn`（清池 + 删注册表行 + 删 `.db`/`-wal`/`-shm`）。返回 `{success:true}`。 |
| `db` | `list` | `token` | `Permission::Db(Db::List)` on `Scope::Global` | 内联鉴权（非 `check_db_permission`）；返回 `list_all()` 结果。返回 `{success, data:[DbInfo...]}`。 |
| `db` | `exec_sql` | `token, name, sql, params: Option<Value>` | `Permission::Db(Db::ExecSql)` on `Scope::Db(name)` | 在命名用户库上 `query_all_raw`，截断 10_000 行。返回 `{success, data:[...], row_count, truncated}`。 |
| `nodeget-server` | `exec_sql` | `token, sql, params: Option<Value>` | `Permission::NodeGet(NodeGetPermission::ExecSql)` on `Scope::Global`（**全信任**） | 在**主库**执行任意 SQL；SQLite 下 `ATTACH DATABASE` 可读写 server uid 下任意文件。截断 10_000 行。返回 `{success, data, row_count, truncated}`。 |
| `nodeget-server` | `get_database_type` | `token` | `Permission::NodeGet(NodeGetPermission::ExecSql)` on `Scope::Global` | 映射 `DbBackend` 为 `"sqlite"`/`"postgres"`/`"mysql"`/`"unknown"`。返回 `{success, data: type}`。 |
| `nodeget-server` | `database_storage` | `token` | **Super token**（`provider.check_super_token`） | 主库逐表字节核算（排除 `seaql_migrations`）。Postgres 走 `pg_total_relation_size`；SQLite 走 `dbstat` 聚合。返回 `{tables: BTreeMap<name,bytes>, total}`。 |

### 鉴权流程

- db 命名空间经 `check_db_permission`（`rpc/db/auth.rs:19`）：`TokenOrAuth::from_full_token` → `get_permission_checker` → `check_token_limit([Scope::Db(name)], [Permission::Db(perm)])`；拒绝返回 `PermissionDenied`（code 102）。`list` 使用 `Scope::Global + Permission::Db(List)`（内联，不走 `check_db_permission`）。
- nodeget 命名空间：`exec_sql`/`get_database_type` 用 `Scope::Global + Permission::NodeGet(NodeGetPermission::ExecSql)`；`database_storage` 用 super token（`check_super_token`，id=1 常量时间比较）。
- 所有错误经 `NodegetError` → `anyhow` → `to_rpc_error` → jsonrpsee `ErrorObject`（携带 nodeget error code）。
- 响应 JSON 形状一致：create/read/update/list 为 `{"success": true, "data": ...}`；exec_sql 为 `{"success": true, "data":[], "row_count": n, "truncated": bool}`（截断阈值硬编码 **10_000** 行）。

## 数据库实体 / 迁移

### 实体表

| 表名 | 列 | 约束 / 索引 / 关系 | 备注 |
|---|---|---|---|
| `crontab` | `id` PK/UNIQUE i64, `name` UNIQUE String, `enable` bool, `cron_expression` String, `cron_type` JsonBinary NOT NULL, `last_run_time` Option<i64> | `cron_type` 不可空 | `entity/crontab.rs:8` |
| `crontab_result` | `id` PK/UNIQUE i64, `cron_id` i64, `cron_name` String, `relative_id` Option<i64>, `run_time` Option<i64>, `success` Option<bool>, `message` Option<String> | 无 FK，`cron_id` 仅逻辑关联 | `entity/crontab_result.rs:8` |
| `db_registry` | `id` PK i64, `name` UNIQUE String, `db_connections` Option<i32>, `max_lifetime_ms` Option<i64>, `created_at` i64 | 另 `#[derive(Default)]`；追踪用户自建 SQLite 库 | `entity/db_registry.rs:6` |
| `dynamic_monitoring` | `id` PK i64, `uuid_id` i16, `timestamp` i64, `storage_time` Option<i64>, `cpu_data`/`ram_data`/`load_data`/`system_data`/`disk_data`/`network_data`/`gpu_data` JsonBinary NOT NULL | data 列均 NOT-NULL Json；`storage_time` 由 m20260516 加入；`#[allow(clippy::missing_fields_in_debug)]` | 高频（默认 1s）动态监控；`entity/dynamic_monitoring.rs:9` |
| `dynamic_monitoring_summary` | `id` PK i64, `uuid_id` i16, `timestamp` i64, `storage_time` Option<i64>, `cpu_usage`/`gpu_usage` Option<i16>, `used_swap`/`total_swap`/`used_memory`/`total_memory`/`available_memory` Option<i64>, `load_one`/`load_five`/`load_fifteen` Option<i16>, `uptime` Option<i32>, `boot_time` Option<i64>, `process_count` Option<i32>, `total_space`/`available_space`/`read_speed`/`write_speed`/`transmit_speed`/`receive_speed`/`total_received`/`total_transmitted` Option<i64>, `tcp_connections`/`udp_connections` Option<i32> | 所有指标列可空；无 Relation | m20260415 加入的预聚合行；`entity/dynamic_monitoring_summary.rs:6` |
| `js_result` | `id` PK i64, `js_worker_id` i64, `js_worker_name` String, `run_type` String, `start_time`/`finish_time` Option<i64>, `param` Option<JsonBinary>, `result` Option<JsonBinary>, `error_message` Option<String> | `param`/`result` 可空 JSON | `entity/js_result.rs:8` |
| `js_worker` | `id` PK i64, `name` UNIQUE String, `description` Option<String>, `js_script` String, `js_byte_code` Option<Vec<u8>>, `route_name` Option<String>, `env` Option<Json>, `runtime_clean_time` Option<i64>, `max_run_time` Option<i64>（NULL→`DEFAULT_MAX_RUN_TIME_MS=30_000`）, `max_stack_size` Option<i64>（默认 1 MiB）, `max_heap_size` Option<i64>（默认 8 MiB）, `create_at` i64, `update_at` i64 | 限制列由 m20260509_000000 加入；常量定义在 ng-js-runtime | `entity/js_worker.rs:8` |
| `kv` | `id` PK i64, `namespace` String, `key` String, `value` JsonBinary NOT NULL | 实体层无复合唯一约束 | `entity/kv.rs:8` |
| `monitoring_uuid` | `id` PK **i32**（非 i64）, `uuid` Uuid, `soft_delete` bool | `soft_delete` 由 m20260517_000000 加入（取代硬删）；UUID cache 自动复活软删行 | 全表最小 PK 宽度；`entity/monitoring_uuid.rs:6` |
| `static_file` | `id` PK i64, `name` String, `path` String, `is_http_root` bool, `cors` bool, `enable` Option<bool> | 由 m20260531_000000_rename_static_to_static_file 从 `static` 改名；`enable` 由 m20260517_000001 加入 | `entity/static_file.rs:8` |
| `static_monitoring` | `id` PK i64, `uuid_id` i16, `timestamp` i64, `storage_time` Option<i64>, `cpu_data`/`system_data`/`gpu_data` JsonBinary NOT NULL, `data_hash` Vec<u8> | `data_hash` 为 BLOB，供 `StaticHashCache` 对 5 分钟静态样本去重 | `entity/static_monitoring.rs:9` |
| `task` | `id` PK i64, `uuid` Uuid, `token` String, `cron_source` Option<String>, `timestamp` Option<i64>, `success` Option<bool>, `error_message` Option<String>, `task_event_type` JsonBinary NOT NULL, `task_event_result` Option<JsonBinary> | `task_event_result` 可空；查询默认上限 `DEFAULT_LIMIT=1000`（在 ng-task 的 `task.query` RPC 施加） | `entity/task.rs:8` |
| `token` | `id` PK i64, `version` i32, `token_key` UNIQUE String, `token_hash` String, `time_stamp_from`/`time_stamp_to` Option<i64>, `token_limit` JsonBinary NOT NULL, `username` UNIQUE Option<String>, `password_hash` Option<String> | `token_key` 与 `username` 均 UNIQUE（username 可空唯一）；`token_limit` 为 Json `Vec<Limit>`；Token 鉴权用 SHA256 + `"NODEGET"` salt（在 ng-token，非此处） | `entity/token.rs:8` |

### 迁移序列

`Migrator`（`migration/src/lib.rs:3`）实现 `MigratorTrait`，`migrations()` 返回按时间序排列的 19 步 `Vec<Box<dyn MigrationTrait>>`，最旧为 `m20260113_000000_create_monitoring_uuid`，最新为 `m20260608_000000_add_indexes`。迁移通过 `init_db_connection` 中的 `Migrator::up(&db, None)` 在运行时应用。`migration/src/main.rs` 的 CLI `cli::run_cli` **已被注释**，`cargo run -p ng-migration` 为 no-op。

## Crate 内部约定

- **Feature 门控**：`default = []` 仅导出 `entity` 模块（纯类型，agent 安全）。`server` feature 拉入 jsonrpsee/tokio/hex/tracing、sea-orm `with-json`/`with-uuid`、`ng_db_migration` 与 `ng-core/for-server`，并门控 `db_connection`、`db_registry`、`rpc` 模块及其再导出。
- **Edition 2024**，crate 级 `#![warn(clippy::all,pedantic,nursery)]`，全局 allow `cast_sign_loss`/`cast_precision_loss`/`cast_possible_truncation`/`similar_names`（cast 类 lint 工作区级抑制）。
- **实体派生**：统一 `#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]`；`Relation` 为空 enum，`ActiveModelBehavior` 取默认；由 `sea-orm-codegen 2.0` 经 `sea-orm-cli` 生成（生成命令见 CLAUDE.md）。
- **大字段**：一律 `#[sea_orm(column_type = "JsonBinary")]`（存 `Json`）。部分可空（`js_result` 的 `param`/`result`、`task` 的 `task_event_result`），部分 NOT-NULL（`dynamic_monitoring` 各 data 列、`kv.value`、`crontab.cron_type`、`static_monitoring` 的 `cpu_data`/`system_data`/`gpu_data`）。
- **日志 target**：连接/注册表/权限代码用 `"db"`；`rpc_exec!` 成功/失败用 `"rpc"`；storage 查询用 `"server"`；nodeget 命名空间 RPC 用 `"nodeget"`；解析错误用 `"ng_core"`。
- **RPC 写法**：一律 `#[rpc(server, namespace=...)]` + `#[method(name=...)]`，**禁止**手写 `register_method`。所有方法返回 `RpcResult<Box<RawValue>>`。方法体包裹 `async { rpc_exec!(...) }.instrument(info_span!(...))`，span 字段含 `token_key`/`username`/`name`（exec 另含 `sql_len`）。宏内部**不含** `await`。
- **鉴权集中化**：经 `ng_core::permission::permission_checker::get_permission_checker()`；db 命名空间构造 `Scope::Db(name)` + `Permission::Db(perm)`，`list`/`storage`/`exec_sql` 用 `Scope::Global`。错误统一 `NodegetError` → `anyhow` → `to_rpc_error` → jsonrpsee `ErrorObject`（带 nodeget error code）。
- **响应形状一致**：create/read/update/list 为 `{"success": true, "data": ...}`；exec_sql 为 `{"success": true, "data":[], "row_count": n, "truncated": bool}`。截断阈值硬编码 **10_000 行**。
- **中文注释**贯穿；辅助结构（`TableSizeRow`/`TableNameRow`）保持私有；未用 param 结构（`NameParam`/`RenameParam`/`ExecSqlParam`）以 `#[allow(dead_code)]` 占位。

## 注意事项与陷阱

- **维护者必须**保证系统时钟不早于 1970：`now_ms_u64()` 在 `db_registry.rs:60` 调用 `.expect("System time is before UNIX epoch")`，错误时钟会让 `get_conn`/`create_conn`/`cleanup_expired` 与清理循环 panic。
- **切勿**假设 SQLite 连接级 PRAGMA（`busy_timeout`、`foreign_keys`）会在连接池轮换后保持：它们是连接级而非持久化（`db_connection.rs:103`）。仅 WAL/synchronous/cache_size（库级）会持久。轮换连接可能让外键 enforcement 静默失效。
- **切勿**对从旧版本升级的库假设 `auto_vacuum=INCREMENTAL` 已生效：该 PRAGMA 仅在空库时生效，旧库仍为 NONE（`db_connection.rs:92`）。
- **维护者必须**在调用 `DbRegistryManager::create_conn` 前校验 name：该方法用 `format!("sqlite://{db_path}/{name}.db?mode=rwc")` 拼接 URL 而**不**调用 `validate_db_name`（`db_registry.rs:316`）。RPC handler 已校验，但任何新调用方都必须先校验，否则攻击者可控 name 会触发路径穿越（如 `../x`）。
- **维护者必须**让 `seed_from_dbreg` 的内联校验与 `validate_db_name` 保持同步：前者在 `db_registry.rs:121` 手写一遍规则（`len<=128`、`[A-Za-z0-9_.-]`、非 `.`/`..`）；规则漂移会导致 seed 进 RPC 校验会拒绝的条目（或反之）。
- **维护者必须**确认"过期 = 近期无查询活动"语义是否符合预期：`cleanup_expired` 以 `now - last_used >= lifetime_ms` 判定，`last_used` 仅由 `get_conn`/`create_conn` 刷新（`db_registry.rs:243`）。注册后从未查询的库其 `last_used` 冻结于创建/seed 时刻，可能被驱逐；热库则永不过期。
- **切勿**遗忘 `remove_conn` 会**永久删除** `.db`/`-wal`/`-shm` 文件：`cleanup_expired` 对闲置连接调用它（`db_registry.rs:404`），无软删除——被驱逐的用户库数据永久丢失。
- **维护者必须**仅将 `NodeGet::ExecSql` 授予完全受信的操作员，并在最小权限 uid 下运行 server：`nodeget::exec_sql` 是文档化的全信任权限，SQLite 下 `ATTACH DATABASE 'any/path'` 可在 server uid 下读写任意文件（创建/覆盖文件、读取其它 `.db`），绕过 `db_registry` 路径约束（`rpc/nodeget/exec_sql.rs:1`）。此为 by-design，无法禁用。
- **切勿**调整 `create`/`update`/`delete` 中"先校验 name 再鉴权"的顺序：该顺序是刻意的（`rpc/db/create.rs:32`）——使未授权调用方带非法 name 时得到 `InvalidInput`（108）而非 `PermissionDenied`（102），且避免用未校验的 name 构造 `Scope`。`read.rs` 不校验 name（仅鉴权 + 查找）；任何新增的、从 name 构造路径或 `Scope` 的 db 命名空间方法**必须**先校验。
- **维护者必须**告知客户端：`db.update` 第三阶段池刷新失败时**不回滚**（`rpc/db/update.rs:121`）。若 `remove_conn(old)` 成功而 `create_conn(new)` 失败，DB 与文件已一致（已重命名）但新连接不在池中——RPC 仍返回 success（仅 warn），客户端需手动 `create_conn`，否则会撞 "Database not found"。
- **切勿**修改 `NameParam`/`RenameParam`/`ExecSqlParam` 期望改变 wire 格式：它们是 `#[allow(dead_code)]` 占位（`rpc/db/mod.rs:23`），实际反序列化由 jsonrpsee `#[method]` 的原始类型签名决定；要改格式须改 `Rpc` trait 的方法签名。
- **维护者必须**仅通过 server 启动时的 `Migrator::up` 应用迁移：`ng-migration` 二进制的 `main()` 已注释（`migration/src/main.rs:2`），`cargo run -p ng-migration` 为 no-op；手动跑 CLI 须先取消注释。
- **切勿**在存在 `get_db()` 活引用时调用 `take_and_close_db`：它是 `unsafe`，经 `(&raw const DB).cast_mut()` + `(*ptr).take()` 回收（`lib.rs:66`），且 `#[allow(dead_code)]` 无任何调用方。应将全局 DB 视作与进程同寿。
- **维护者必须**警惕 `cleanup_handle` 锁非 poison 容忍：除 `init`（`db_registry.rs:101`）与 `shutdown`（`:464`）对 `cleanup_handle` 用 `.unwrap()` 外，其余锁获取均 `unwrap_or_else(PoisonError::into_inner)`。若任务在持 `cleanup_handle` 时 panic，init/shutdown 会传播 poison panic。
- **维护者必须**对 `exec_sql_inner`（db 命名空间，`rpc/db/exec_sql.rs:30`）与 `nodeget::exec_sql` 同步修改：两者为近似重复代码（分别作用于用户池与主库），截断/参数解析/行映射的任何修复**必须**同时落到两者，否则会漂移；当前无共享的 execute+truncate+to_json 流水线 helper。

## 依赖关系

ng-db 在工作区内被几乎所有业务 crate 依赖（通过 `entity` 模块共享表定义），其中 agent 仅以默认 feature 依赖（纯类型），server 二进制与 ng-infra、ng-token、ng-kv、ng-task、ng-crontab、ng-js-worker、ng-static、ng-monitoring 等启用 `server` feature 的 crate 使用其连接初始化、`DbRegistryManager` 与 RPC handler。ng-db 自身依赖 ng-core（鉴权类型与 `NodegetError`/`permission_checker`）、sea-orm、jsonrpsee（server feature）、以及同包内的 `ng_db_migration`（即 `ng-migration`，提供 `Migrator`）。
