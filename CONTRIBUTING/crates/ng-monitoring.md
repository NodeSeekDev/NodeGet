# ng-monitoring — 监控数据模型、缓存与 RPC 命名空间

> 概览：ng-monitoring 提供 NodeGet 服务端的监控数据模型（static / dynamic / dynamic-summary）、查询 DSL、异步批量写入缓冲区、三个内存 Cache（UUID↔id、last-seen 值、static hash 去重），以及三个 RPC 命名空间（`agent`、`agent-uuid`、`nodeget-server`）。`default = []` 只暴露类型（agent 可安全依赖）；`server` feature 追加 Cache、buffer 与全部 RPC handler。所有 RPC 方法使用 `#[rpc(server, namespace = ...)]` + `#[method(name = ...)]`，返回 `RpcResult<Box<RawValue>>`，server 二进制在启动时把 `rpc_module()` 合并入主 `RpcModule`。

## 模块结构

```
crates/ng-monitoring/src/
├── lib.rs                          # Crate root，gate default/server 模块，导出 rpc_module()
├── data_structure.rs               # 监控数据结构、canonical SHA-256 哈希、聚合过滤器、i16 缩放函数
├── query.rs                        # 查询 DSL（QueryCondition 等）、SCALED_SUMMARY_COLUMNS、apply_descaling_to_json_object
├── monitoring_buffer.rs            # server：三条 mpsc + 三个 flush_loop 的异步批量写入缓冲区
├── monitoring_last_cache.rs        # server：每 UUID last-seen JSON 缓存（hand-rolled OnceLock）
├── monitoring_uuid_cache.rs        # server：UUID↔i16 id 双向缓存（make_global_cache! + 软删）
├── static_hash_cache.rs            # server：每 uuid_id 的 static data_hash 前 16 字节去重缓存
└── rpc/                            # server：RPC 命名空间实现
    ├── agent/
    │   ├── mod.rs                  # `agent` 命名空间 trait + AgentRpcImpl（12 个方法）
    │   ├── report_dynamic.rs       # 动态数据上报
    │   ├── report_dynamic_summary.rs
    │   ├── report_static.rs        # 两级去重
    │   ├── query_dynamic.rs        # 字段级权限、流式 JSON
    │   ├── query_dynamic_summary.rs
    │   ├── query_static.rs
    │   ├── query_dynamic_multi_last.rs        # 批量最新值，部分命中合并
    │   ├── query_dynamic_summary_multi_last.rs
    │   ├── query_static_multi_last.rs
    │   ├── delete_common.rs        # 共享：scopes_from_conditions、limit clamp、resolve_conditions
    │   ├── delete_dynamic.rs
    │   ├── delete_dynamic_summary.rs
    │   └── delete_static.rs
    ├── agent_uuid/
    │   ├── mod.rs                  # `agent-uuid` 命名空间（list_all / list_all_with_agent_mode / delete）
    │   ├── list_all.rs
    │   ├── list_all_with_agent_mode.rs
    │   └── delete.rs
    └── nodeget/
        ├── mod.rs                  # `nodeget-server` 命名空间贡献（注意使用 ng_db::rpc_exec!）
        └── list_all_agent_uuid.rs  # 分层权限过滤可见 UUID
```

`lib.rs:28` 声明 `mod data_structure; mod query;` 永远编译；`monitoring_buffer` / `monitoring_last_cache` / `monitoring_uuid_cache` / `static_hash_cache` / `rpc` 仅在 `#[cfg(feature = "server")]` 下编译。

## 公共 API

| 名称 | 签名 | 行为 |
|---|---|---|
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<()>`（`cfg(feature="server")`，`lib.rs:58`） | 构造空 `RpcModule<()>`，依次合并 `AgentRpcImpl`、`AgentUuidRpcImpl`、`NodegetServerRpcImpl`（`into_rpc()`）；每次 `merge()` 使用 `.expect()`，命名冲突即 panic。 |
| `data_structure` 类型 | `StaticMonitoringData`、`DynamicMonitoringData`、`DynamicMonitoringSummaryData`（含子结构）、`DiskKind` | 全部 `derive(Serialize, Deserialize)`；`StaticMonitoringData::compute_data_hash`、`DynamicMonitoringSummaryData::from_with_filter`、`impl From<&DynamicMonitoringData>`。 |
| `StaticMonitoringData::compute_data_hash` | `pub fn compute_data_hash(cpu:&StaticCPUData, system:&StaticSystemData, gpu:&[StaticGpuData]) -> Result<Vec<u8>, NodegetError>`（`data_structure.rs:56`） | 对三个字段的 canonical JSON 做 SHA-256，返回前 16 字节；仅在 serde 失败时出错。 |
| `DynamicMonitoringSummaryData::from_with_filter` | `pub fn from_with_filter(data, select_disk:Option<&[String]>, select_network_interface:Option<&[String]>) -> Self`（`data_structure.rs:387`） | `None`/空 → 默认排除；`Some`（非空）→ 仅汇总匹配项；做缩放与聚合。 |
| `is_virtual_interface` / `is_excluded_mount` / `is_excluded_file_system` | `pub fn(&str) -> bool` | 用于 agent 汇总逻辑，可复用。 |
| `is_excluded_summary_disk` | `pub fn(&DynamicPerDiskData) -> bool` | 仅用于 summary 默认磁盘排除口径；内部按 `mount_point` / `file_system` 判断。 |
| `query` 类型 | `QueryCondition`、`StaticDataQuery`、`DynamicDataQuery`、`DynamicSummaryQuery`、`DynamicSummaryQueryField`、`StaticResponseItem`、`DynamicResponseItem`、`DynamicSummaryResponseItem`（其中 `StaticDataQueryField`/`DynamicDataQueryField` 从 ng-core 再导出） | 查询 DSL；见「关键类型与常量」。 |
| `apply_descaling_to_json_object` | `pub fn apply_descaling_to_json_object(obj:&mut serde_json::Map<String,Value>)`（`query.rs:308`） | 对 scaled 字段做 `/10.0`，每行必须调用**恰好一次**。 |
| `SCALED_SUMMARY_COLUMNS` | `pub const &[&str] = &["cpu_usage","load_one","load_five","load_fifteen"]`（`query.rs:295`） | ×10 缩放字段的单一事实源。 |
| `monitoring_buffer::init` / `get` / `flush_and_shutdown` | `pub fn init(config:Option<&MonitoringBufferConfig>)`；`pub fn get()->Option<&'static MonitoringBuffers>`；`pub async fn flush_and_shutdown()` | 初始化全局单例、获取之、或关闭发送端并在 5s 内 join flush 循环。 |
| `MonitoringBuffers` 字段 + `BufferSender::send` / `dropped_count` | `pub fn send(&self, item:T)`；`pub fn dropped_count(&self)->u64` | `send()` 用 `try_send`（非阻塞）；`dropped_count()` 统计 `try_send` 失败导致的丢弃数（典型为 channel 满）。显式 `close()` 之后的 fast-path send 会静默忽略，不计入 dropped。 |
| `MonitoringLastCache::init` / `global` / `update_*` / `get_*` | 见签名；`update_static_prebuilt` / `update_dynamic_prebuilt` / `update_dynamic_summary_prebuilt` 接受预构建 JSON `Value`；`get_*_last` 返回过滤后 `Value`，`get_*_last_raw` 返回预序列化 `Arc<str>`（summary 已反缩放） | hand-rolled `OnceLock` 单例（`monitoring_last_cache.rs`）。 |
| `MonitoringUuidCache`（DbBackedCache）方法 | `init/global/reload`（宏生成）；`get_id(&Uuid)->Option<i16>`；`get_uuid(i16)->Option<Uuid>`；`is_active(&Uuid)->bool`；`exists(&Uuid)->bool`；`list_all()->Vec<Uuid>`；`list_all_with_agent_mode()->Vec<(Uuid,bool)>`；`async get_or_insert(Uuid)->Result<i16,NodegetError>`；`async soft_delete(Uuid)->Result<bool,NodegetError>` | `make_global_cache!` 提供 init/global/reload；`get_or_insert` 复活软删行并处理 UNIQUE 冲突。 |
| `StaticHashCache::init` / `global` / `is_duplicate` / `update` | `pub fn init()`；`pub fn global()->Option<&'static Self>`；`pub fn is_duplicate(&self,i16,&[u8])->bool`；`pub fn update(&self,i16,&[u8])` | hand-rolled OnceLock；存每 `uuid_id` 的前 16 字节 hash。 |
| `AgentRpcImpl` / `AgentUuidRpcImpl` / `NodegetServerRpcImpl` | `pub struct ... ;`（`server` feature） | 空 struct，实现 `RpcHelper`；`rpc_module()` 内部实例化，外部一般不直接用。 |

## 关键类型与常量

### 监控数据结构（`data_structure.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `StaticMonitoringData` | `data_structure.rs:12` | `{ uuid:Uuid, time:u64, data_hash:Vec<u8>, cpu:StaticCPUData, system:StaticSystemData, gpu:Vec<StaticGpuData> }`；`data_hash` 为 canonical(cpu,system,gpu) 的 SHA-256 前 16 字节。 |
| `StaticMonitoringData::compute_data_hash` | `data_structure.rs:56` | 序列化三个字段为 `serde_json::Value`，用 `WriteToDigest` 适配器以 `\n` 分隔流式写入 `Sha256`，返回前 16 字节；无论原始 JSON 键序都确定。 |
| `WriteToDigest<'a>(&'a mut Sha256)` | `data_structure.rs:92` | 实现 `io::Write`，每次 `write` 调用 `Digest::update`；零分配。 |
| `write_canonical_json` | `data_structure.rs:110` | 对 `Object` 键 `sort_unstable`，`Array` 保持原序，标量走 `serde_json::to_writer`；确定性输出。 |
| `u64_to_i64_saturating` / `u64_to_i32_saturating` | `data_structure.rs:30` | u64→i64/i64→i32 饱和转换，溢出返回 `i64::MAX` 而非回绕；用于所有字节类字段。 |
| `DynamicMonitoringData` | `data_structure.rs:148` | `{ uuid, time:u64, cpu, ram, load, system, disk:Arc<Vec<DynamicPerDiskData>>, network, gpu:Arc<Vec<DynamicGpuData>> }`；disk/gpu 用 `Arc` 包裹以 O(1) clone。 |
| `DynamicMonitoringSummaryData` | `data_structure.rs:175` | `uuid/time` 外加 23 个 `Option` 字段；`cpu_usage`/`load_one`/`load_five`/`load_fifteen` 为 `i16`（实际值 = stored/10.0），`gpu_usage` 为普通 `i16`（0–100），字节字段为 `i64`。 |
| `VIRTUAL_INTERFACE_PREFIXES` | `data_structure.rs:230` | `const &[&str] = ["br","cni","docker","podman","flannel","lo","veth","virbr","vmbr","tap","fwbr","fwpr"]`；供 `is_virtual_interface()` 使用。 |
| `EXCLUDED_MOUNT_PREFIXES` | `data_structure.rs:236` | 包含 `/tmp`、`/dev`、`/run`、`/var/lib/containerd`、`/var/lib/containers`、`/var/lib/docker`、`/var/lib/kubelet/*`、`/var/lib/rancher/k3s/agent/...`、`/proc`、`/sys`、`/sys/fs/cgroup`、`/etc/hosts` 等。 |
| `EXCLUDED_FILE_SYSTEMS` | `data_structure.rs:262` | `autofs,bpf,cgroup,cgroup2,debugfs,devtmpfs,fusectl,nsfs,overlay,proc,pstore,securityfs,squashfs,sysfs,tmpfs,tracefs`。 |
| `is_virtual_interface` | `data_structure.rs:286` | `name` 是否以任一 `VIRTUAL_INTERFACE_PREFIXES` 前缀开头。 |
| `mount_point_matches_prefix` | `data_structure.rs:296` | 仅精确匹配或路径段前缀（后跟 `/`）为真；故 `/run` 不匹配 `/runner`。 |
| `is_excluded_mount` | `data_structure.rs:308` | 任一 `EXCLUDED_MOUNT_PREFIXES` 命中。 |
| `is_excluded_file_system` | `data_structure.rs:319` | `eq_ignore_ascii_case` 成员判断（大小写不敏感）。 |
| `is_excluded_summary_disk` | `data_structure.rs:329` | `is_excluded_mount(mount_point) || is_excluded_file_system(file_system)`；仅用于 summary 聚合。 |
| `scale_cpu_percent_to_i16` | `data_structure.rs:347` | `(percent*10).clamp(0,1000) as i16`；NaN/±Inf 返回 `None`；钳制到 0..=100.0%。 |
| `scale_load_to_i16` | `data_structure.rs:371` | `(load*10).clamp(i16::MIN, i16::MAX) as i16`；允许 >100 的 load（高核机器）。 |
| `DynamicMonitoringSummaryData::from_with_filter` | `data_structure.rs:387` | 非空过滤 → 仅汇总匹配 disk/interface；空/None → 默认排除；聚合空间与网络总量，`cpu_usage` 经 `scale_cpu_percent_to_i16`，`gpu_usage` 取首个 GPU `utilization_gpu`。 |
| `From for DynamicMonitoringSummaryData` | `data_structure.rs:458` | 委派给 `from_with_filter(data, None, None)`。 |
| `DiskKind` | `data_structure.rs:570` | `enum { Hdd, Ssd, Unknown }`；serde 默认 PascalCase。 |
| `DynamicPerDiskData` 等 | `data_structure.rs:581` | `{ kind, name, file_system, mount_point, total_space, available_space, is_removable, is_read_only, read_speed, write_speed }`；同样定义 `DynamicPerNetworkInterfaceData`、`DynamicGpuData` 等。 |

### 查询 DSL（`query.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `QueryCondition` | `query.rs:15` | `enum { Uuid(Uuid), TimestampFromTo(i64,i64), TimestampFrom(i64), TimestampTo(i64), StorageTimeFromTo(i64,i64), StorageTimeFrom(i64), StorageTimeTo(i64), Limit(u64), Last }`；`#[serde(rename_all="snake_case")]`；所有时间为毫秒。 |
| `StaticDataQuery` | `query.rs:41` | `{ fields:Vec<StaticDataQueryField>, condition:Vec<QueryCondition> }`（从 ng-core 再导出）。 |
| `StaticResponseItem` | `query.rs:59` | `{ uuid:Uuid, timestamp:i64, cpu/system/gpu:Option<Value> with skip_serializing_if=Option::is_none }`；仅 Serialize。 |
| `DynamicSummaryQueryField` | `query.rs:108` | `enum (Copy)`，23 个 summary 字段；`column_name()` / `json_key()`（== column_name）/ `is_scaled()`。 |
| `DynamicSummaryQueryField::is_scaled` | `query.rs:198` | `pub fn -> bool`；成员判定 `SCALED_SUMMARY_COLUMNS`；测试断言其与 const 对每个 variant 一致（单一事实源不变量）。 |
| `SCALED_SUMMARY_COLUMNS` | `query.rs:295` | `pub const &[&str] = ["cpu_usage","load_one","load_five","load_fifteen"]`；新增缩放列只需改此。 |
| `apply_descaling_to_json_object` | `query.rs:308` | 对每个 `SCALED_SUMMARY_COLUMNS` 键：若为 Number，解析为 f64 并 `/10.0`；若结果可有限表示则替换，否则不变；Null/非 Number 不动；**必须每行恰好调用一次**。 |

### 缓冲区（`monitoring_buffer.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `DEFAULT_FLUSH_INTERVAL_MS` / `DEFAULT_MAX_BATCH_SIZE` / `DEFAULT_CHANNEL_CAPACITY` | `monitoring_buffer.rs:25` | `u64 = 500` / `1000` / `10000`；`MonitoringBufferConfig` 字段为 `None` 时使用。 |
| `BUFFERS` | `monitoring_buffer.rs:34` | `static OnceLock<MonitoringBuffers>`，全局单例。 |
| `FLUSH_HANDLES` | `monitoring_buffer.rs:37` | `static Mutex<Option<[JoinHandle<()>;3]>>`；`unwrap()` 在中毒时 panic（已文档化）。 |
| `MonitoringBuffers` | `monitoring_buffer.rs:40` | `{ static_mon, dynamic_mon, dynamic_summary }` 全为 `BufferSender<…ActiveModel>`。 |
| `init` | `monitoring_buffer.rs:65` | 读取可选 `flush_interval_ms`/`max_batch_size`/`channel_capacity`（回退默认），建三条 mpsc，存入 `BUFFERS`（已 init 则 warn+sink），spawn 三个 `flush_loop`（硬编码 `num_columns` 8/11/27），存 handle。 |
| `flush_and_shutdown` | `monitoring_buffer.rs:147` | 对三个 sender `close()`（触发 rx 收尾 flush），随后 `tokio::time::timeout(5s, join_all(handles))`；逐任务记录 panic 或超时；`BUFFERS` 未初始化时 no-op。 |
| `BufferSender<T>` | `monitoring_buffer.rs:182` | `{ tx:Mutex<Option<Sender<T>>>, cap:usize, dropped:AtomicU64, closed:AtomicBool }`；`send()` 用 `try_send`（非阻塞），仅在 `try_send` 失败时累加 `dropped` 并 warn；显式 `close()` 后的 fast-path send 直接返回；`close()` 本身幂等。 |
| `BufferSender::send` | `monitoring_buffer.rs:207` | 快速路径：`closed.load(Acquire)` 为真直接返回；否则锁 `tx` 并 `try_send`。 |
| `BufferSender::dropped_count` | `monitoring_buffer.rs:233` | 累计丢弃数，`Relaxed` load。 |
| `flush_loop` | `monitoring_buffer.rs:266` | `tokio::select! {biased; rx.recv() | ticker.tick()}`；`recv` Some 时 push 并排空至 `max_batch_size`；`recv` None 时 flush 余量并返回。 |
| `SQLITE_MAX_VARIABLE_NUMBER` | `monitoring_buffer.rs:322` | `const usize = 999`。 |
| `do_flush` | `monitoring_buffer.rs:333` | SQLite：`chunk_size = 999/num_columns.max(1)`（static 124 / dynamic 90 / summary 37）；PostgreSQL：`chunk_size == total`；每子批失败丢弃并 error-log，汇报 inserted/dropped。 |

### last-seen 缓存（`monitoring_last_cache.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `CachedEntry` | `monitoring_last_cache.rs:36` | `{ value:serde_json::Value, serialized:Option<Arc<str>> }`；`serialized` 在 `to_string` 失败时为 `None`，raw getter 返回 `None`；summary 的 `value` 存 scaled、`serialized` 存 descaled。 |
| `CachedEntry::new` | `monitoring_last_cache.rs:46` | 预序列化为 `Arc<str>`（失败为 `None`）。 |
| `CachedEntry::new_summary` | `monitoring_last_cache.rs:57` | clone value，对其应用 `apply_descaling_to_json_object` 后序列化写入 `serialized`；`value` 保留 scaled。 |
| `MonitoringLastCache` | `monitoring_last_cache.rs:72` | `{ static_cache, dynamic_cache, dynamic_summary_cache }` 全为 `RwLock<HashMap<Uuid, CachedEntry>>`；`#[allow(clippy::struct_field_names)]`。 |
| `MonitoringLastCache::init` / `global` | `monitoring_last_cache.rs:83` | `get_or_init` 容量 32 的 `HashMap`。 |
| `update_static_prebuilt` | `monitoring_last_cache.rs:97` | 包成 `CachedEntry::new` 写入 `static_cache`。 |
| `update_dynamic_summary_prebuilt` | `monitoring_last_cache.rs:111` | 用 `CachedEntry::new_summary`（反缩放 serialized）。 |
| `get_static_last` | `monitoring_last_cache.rs:144` | 按 `&[StaticDataQueryField]` 经 `build_filtered_map` 构建过滤 map（恒含 uuid+timestamp）；未命中返回 `None`。 |
| `get_dynamic_summary_last` | `monitoring_last_cache.rs:184` | 字段空 → 返回完整克隆（SCALED，调用方负责反缩放）；否则过滤 map；**不反缩放**。 |
| `get_static_last_raw` / `get_dynamic_last_raw` / `get_dynamic_summary_last_raw` | `monitoring_last_cache.rs:211` | 返回预序列化完整对象（summary 已反缩放）；缺失或原序列化失败返回 `None`。 |
| `build_filtered_map` | `monitoring_last_cache.rs:252` | 恒插入 uuid/timestamp（若存在），随后是 extra keys；`serde_json::Map` 底层 BTreeMap → 字母序。 |
| `recover_read` / `recover_write` | `monitoring_last_cache.rs:277` | 锁中毒时 `into_inner` 并 warn，永不中止 cache。 |
| `build_static_value` | `monitoring_last_cache.rs:303` | 5 容量 Map；尽力而为，序列化失败字段静默跳过。 |
| `build_static_value_prebuilt` | `monitoring_last_cache.rs:339` | 复用调用方预序列化 `Value`（report_static 已提前错误处理）。 |
| `build_dynamic_value_prebuilt` | `monitoring_last_cache.rs:417` | 9 容量 Map。 |
| `build_dynamic_summary_value` | `monitoring_last_cache.rs:454` | 24 容量 Map，用 `opt_field!` 仅插入 `Some`；**保留 scaled**，反缩放在查询时或 `CachedEntry::new_summary`。 |

### UUID 缓存（`monitoring_uuid_cache.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `MonitoringUuidCacheInner` | `monitoring_uuid_cache.rs:19` | `{ by_uuid:HashMap<Uuid,(i16,bool)>, by_id:HashMap<i16,(Uuid,bool)> }`，`bool` 为 `soft_delete`。 |
| `MonitoringUuidCache` | `monitoring_uuid_cache.rs:27` | `{ inner:RwLock<MonitoringUuidCacheInner> }`。 |
| `make_global_cache!` 调用 | `monitoring_uuid_cache.rs:53` | `ng_infra::make_global_cache!(MonitoringUuidCache, MONITORING_UUID_CACHE_GLOBAL)` → `OnceLock` 支撑的 init/global/reload（全表 reload）。 |
| `DbBackedCache impl` | `monitoring_uuid_cache.rs:55` | `Model = monitoring_uuid::Model`；`cache_name = "monitoring_uuid"`；`build_cache` 建 by_uuid/by_id（id 转 i16）；`reload_from_models` 原子替换；`load_all` 委派 `load_from_db::<monitoring_uuid::Entity>()`。 |
| `get_id` / `get_uuid` | `monitoring_uuid_cache.rs:109` / `:117` | 忽略 `soft_delete` 的双向查找。 |
| `is_active` | `monitoring_uuid_cache.rs:125` | 存在且未软删。 |
| `exists` | `monitoring_uuid_cache.rs:133` | `contains_key`（含软删）。 |
| `list_all` | `monitoring_uuid_cache.rs:138` | 非软删 UUID，已排序。 |
| `list_all_with_agent_mode` | `monitoring_uuid_cache.rs:152` | 全部 UUID 带 `soft_delete` 标志，按 UUID 排序。 |
| `get_or_insert` | `monitoring_uuid_cache.rs:177` | 1) cache 命中 active → 返回；2) DB 按 uuid 查；3a) 命中且软删 → `update soft_delete=false`（复活）；3b) 命中且 active → 仅更新 cache；4) 未命中 → INSERT（`ActiveValue::default` id、`Set(uuid)`、`Set(false)`），UNIQUE 冲突（并发首注册）则回退 SELECT 取 id（INSERT-OR-IGNORE 语义）；错误 → `NodegetError::DatabaseError`。 |
| `soft_delete` | `monitoring_uuid_cache.rs:283` | uuid 缺失返回 `false`，已软删返回 `true`，否则置 `soft_delete=true` 并更新两 map；幂等。 |

### static hash 缓存（`static_hash_cache.rs`）

| 项 | 行 | 说明 |
|---|---|---|
| `Inner` | `static_hash_cache.rs:12` | `{ by_uuid_id:HashMap<i16,[u8;16]> }`；每条目固定 16 字节栈数组。 |
| `StaticHashCache` | `static_hash_cache.rs:18` | `{ inner:RwLock<Inner> }`；init 容量 32。 |
| `truncate_to_16` | `static_hash_cache.rs:43` | 复制 `min(len,16)` 字节并零填充；写端（`compute_data_hash` 返回 16 字节）与读端一致。 |
| `init` / `global` | `static_hash_cache.rs:52` | hand-rolled `OnceLock`。 |
| `is_duplicate` | `static_hash_cache.rs:70` | 截断 hash 相等即真。 |
| `update` | `static_hash_cache.rs:82` | 插入截断 hash；在 `report_static` 非重复 insert 后调用（含 slow-path DB 命中时）。 |

## 内部机制

### Write path（report）
`report_*` RPC → `MonitoringUuidCache::get_or_insert`（UUID→i16）→ 构建 `ActiveModel` → `BufferSender::send`（`try_send`，非阻塞）→ mpsc → `flush_loop` 在 tick/容量触发时排空 → `do_flush` 分块 `insert_many`。同时 `report_*` 更新 `MonitoringLastCache`（static 额外更新 `StaticHashCache`），使 multi-last 查询优先命中内存。

### Multi-last 查询的部分命中合并
对每个请求的 uuid：先查 `MonitoringLastCache`（all-fields 走 raw `Arc<str>` 快路径，否则过滤 `Value`）；未命中连同原始下标收集，构建单条 `UNION ALL` `Statement` 流式执行，按 index `zip` 回填结果向量；最后将 cache Raw 与 DB `Value` 序列化进同一 JSON 数组缓冲。`zip` 防御 DB 返回行少于未命中数（uuid 已注册但尚无数据）。

### Summary 反缩放的双重表示
`MonitoringLastCache::new_summary` clone value，应用 `apply_descaling_to_json_object` 后序列化写入 `serialized`（供 `get_dynamic_summary_last_raw`，已反缩放）；`value` 保留 scaled 供过滤查询，过滤 multi-last 路径调用 `descale_cached_summary` 反缩放后再返回。DB 流式路径对每行应用 `apply_descaling_to_json_object`。

### Static 两级去重
`report_static` 先查 `StaticHashCache::is_duplicate`（内存中前 16 字节 hash）→ 未命中则按 `(uuid_id, data_hash)` 查 DB（覆盖 cache 尚未填充的历史）→ 确认是新数据后才经 buffer 插入并更新 hash cache。两级去重是承重的，切勿移除 DB 回退路径。

### 并发 UUID 注册
`get_or_insert` 处理两并发首注册同一 UUID 同时通过「不在 DB」检查的竞争：一方 INSERT 成功，另一方得到 `SqlErr::UniqueConstraintViolation`，经 `sql_err()` 匹配捕获后回退 SELECT 取回真实 id（INSERT-OR-IGNORE 语义），镜像 `super_token::generate_super_token`。

### Buffer flush 调度与关闭
`flush_loop` 使用 `tokio::select!{biased; rx.recv() | ticker.tick()}`；channel 关闭（`recv=None`）时 flush 余量并返回。`flush_and_shutdown` 先 `close()` 发送端（drop 内部 Sender → channel 关闭 → rx 收尾 flush），再 `join_all` 附 5s 超时。`MissedTickBehavior::Delay` 防止 tick 爆发。

### SQLite 999 参数分块
`do_flush` 检测 SQLite 后端并按 `chunk_size = 999/num_columns`（static/8=124、dynamic/11=90、summary/27=37）切分，以低于 SQLite 999 bind 上限；PostgreSQL 写入整批。每子批失败仅丢弃该子批并 error-log。

### Permission 注入分裂（crate 局部不一致）
`report_static`、`report_dynamic_summary`、全部 multi-last/query 走 `ng_token::get::check_token_limit`；`delete_dynamic`、`delete_dynamic_summary` 走 `require_permission_checker()` + 注入的 `PermissionChecker` trait（ng-core 定义，server 在启动时注册）；`delete_static` 又用 `check_token_limit`。维护者必须意识到此分裂。

### Error object 构造
所有 RPC 方法把方法体包进内部 `async {}` 块，`.await`，Err 时从 `anyhow_to_nodeget_error` 构建 jsonrpsee `ErrorObject::owned`；外层 `rpc_exec!`（在 mod.rs）追加日志。查询路径还经 `anyhow_error_to_raw` 附带结构化 `RawValue` 载荷。

### Limit 钳制
`delete_common::extract_limit_and_last` 把 `Limit` 钳到 `10_000`，避免选出巨大的 `Vec<i64>` id 列表导致 OOM；常规 query RPC（`query_dynamic` / `query_static` / `query_dynamic_summary`）也把 `Limit` 钳到 `MAX_LIMIT=10_000`，默认 `capacity_hint` 为 100（static）/ 5000（dynamic、summary）。multi-last RPC 参数是 `Vec<Uuid>` + `fields`，当前没有 `Limit` 参数，也没有单独的 UUID 数量钳制。

## RPC 方法

### `agent` 命名空间（12 个方法，`rpc/agent/mod.rs:39`）

| 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|
| `report_dynamic` | `token:String, dynamic_monitoring_data:DynamicMonitoringData` | `DynamicMonitoring::Write` 限 `Scope::AgentUuid(uuid)`（`check_token_limit`） | `get_or_insert` uuid→id；序列化 7 字段为 `Value`（失败报错）；`ActiveModel`（`Set` clone Value，原值移入 `cache_value` 避免重复 `to_value`）；发送 `dynamic_mon` buffer；更新 dynamic last-cache；返回 `{"status":"buffered"}`。 |
| `report_dynamic_summary` | `token:String, data:DynamicMonitoringSummaryData` | `DynamicMonitoringSummary::Write` + AgentUuid | 字段直接来自 data（已 scaled）；发送 summary buffer；经 `build_dynamic_summary_value` 更新 last-cache（保留 scaled，`new_summary` 预反缩放 raw）；返回 buffered。 |
| `report_static` | `token:String, static_monitoring_data:StaticMonitoringData` | `StaticMonitoring::Write` + AgentUuid | 两级去重：`StaticHashCache::is_duplicate` 命中 → `{"status":"skipped","reason":"duplicate_hash"}`；未命中则按 `(uuid_id, data_hash)` DB 查，命中则更新 hash cache 并 skipped；否则构建 ActiveModel（`storage_time` 经 `get_local_timestamp_ms_i64`）发送 buffer、更新 hash cache，返回 buffered。 |
| `query_dynamic` | `token:String, dynamic_data_query:DynamicDataQuery` | `DynamicMonitoring::Read` 按请求字段（空字段 = 任一 7 字段 any_allowed） | 构建 scopes（每 Uuid 条件一个 AgentUuid，否则 Global）；先经 cache 把 UUID 解析为 id（未知 NotFound）；`select_only` 含 uuid_id+timestamp+请求列；`execute_query` 流式构建 JSON 数组，含 uuid_id→uuid 翻译与 `rename_and_fix_json`；`capacity_hint` = clamp 或 5000，buffer = hint*200 字节。 |
| `query_dynamic_summary` | `token:String, query:DynamicSummaryQuery` | `DynamicMonitoringSummary::Read`（单一，非按字段） | 空字段选全 23 列；否则 `field_to_column`；每行应用 `apply_descaling_to_json_object`；`capacity_hint` = clamp 或 5000。 |
| `query_static` | `token:String, static_data_query:StaticDataQuery` | `StaticMonitoring::Read` 按字段（空 = 任一 cpu/system/gpu） | 3 字段；`capacity_hint` = clamp 或 100（static 数据小）。 |
| `dynamic_data_multi_last_query` | `token:String, uuids:Vec<Uuid>, fields:Vec<DynamicDataQueryField>` | `DynamicMonitoring::Read` 按字段（空 = 任一 7），每 uuid AgentUuid 作用域 | 去重 uuid（保序）；all-fields → `get_dynamic_last_raw`（`DynamicResult::Raw(Arc<str>)`），否则过滤 `Value`；未命中构建 `UNION ALL`（内层 `ORDER BY timestamp DESC LIMIT 1`，外层包 alias 子查询以兼容 UNION），按 index `zip` 合并；返回 JSON 数组。 |
| `dynamic_summary_multi_last_query` | `token:String, uuids:Vec<Uuid>, fields:Vec<DynamicSummaryQueryField>` | `DynamicMonitoringSummary::Read` | `fields.is_empty()` 即 all-fields；过滤 cache 命中经 `descale_cached_summary` 反缩放；DB 行经 `apply_descaling_to_json_object`。 |
| `static_data_multi_last_query` | `token:String, uuids:Vec<Uuid>, fields:Vec<StaticDataQueryField>` | `StaticMonitoring::Read` 按字段（空 = 任一 3） | 部分命中合并；all-fields 用 `get_static_last_raw`。 |
| `delete_dynamic` | `token:String, conditions:Vec<QueryCondition>` | `DynamicMonitoring::Delete`（**经 `require_permission_checker()` 注入**） | Limit/Last → 选 id（DESC，钳 10_000）后 `delete_many WHERE id IN (...)`；否则带过滤 `delete_many`；返回 `{success,deleted,condition_count}`。 |
| `delete_dynamic_summary` | `token:String, conditions:Vec<QueryCondition>` | `DynamicMonitoringSummary::Delete`（注入 checker） | 同上，针对 summary 表。 |
| `delete_static` | `token:String, conditions:Vec<QueryCondition>` | `StaticMonitoring::Delete`（**直接 `check_token_limit`，非注入 checker**） | 同 select-ids/delete_many 形式。 |

`agent` 命名空间鉴权流程：每个方法在 mod.rs 用 `token_identity(&token)` 提取 `(token_key, username)` 作为 span 字段，建 `info_span!(target:"monitoring", ...)`，instrument async 块，内部 `rpc_exec!(submodule_fn(...).await)`。

### `agent-uuid` 命名空间（3 个方法，`rpc/agent_uuid/mod.rs:20`）

| 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|
| `list_all` | `token:String` | `MonitoringUuid::List` + `Scope::Global` | 返回 `cache.list_all()`（仅 active，已排序）的 JSON 数组。 |
| `list_all_with_agent_mode` | `token:String` | `MonitoringUuid::List` + Global | 返回 `Vec<{uuid, soft_delete}>`（`AgentUuidWithMode` 序列化结构，`list_all_with_agent_mode.rs:16`）。 |
| `delete` | `token:String, agent_uuid:Uuid` | `MonitoringUuid::Delete` + Global（**文档注释写「需 SuperToken」，但代码并不强制**） | 调 `cache.soft_delete(uuid)`；返回 `{success:true,message:"Agent UUID soft-deleted"}` 或 `{success:false,message:"Agent UUID not found"}`。 |

`agent-uuid` 命名空间的 span target 为 `"server"`（`info_span!(target:"server", "agent-uuid::...")`）。

### `nodeget-server` 命名空间（1 个方法，`rpc/nodeget/mod.rs:17`）

| 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|
| `list_all_agent_uuid` | `token:String` | Super-token，或（`NodeGet::ListAllAgentUuid` 或 `MonitoringUuid::List`）全局，或 scoped（对该 AgentUuid 同时具备 List 与某操作权限） | 分层权限解析后过滤 `cache.list_all()`（active），返回 `{"uuids":[...]}`。 |

`nodeget-server` 模块使用 `ng_db::rpc_exec!`（注意：不是 `ng_infra::rpc_exec!`，见 `rpc/nodeget/mod.rs:11`）。server 二进制会把其他 crate 的方法合并进同一 `nodeget-server` 命名空间。

分层权限模型（`list_all_agent_uuid.rs:101`）：super-token → `All`；否则取 token limits，校验 `timestamp_from/to`；收集 List 权限（`NodeGet::ListAllAgentUuid` 或 `MonitoringUuid::List`）—— Global → `has_global_list_permission`（→ All），AgentUuid scopes → `nodeget_scoped_uuids`；另收集 `operable_scoped_uuids`（带任一非 List 权限的 AgentUuid scopes）；无任何 List 权限 → `PermissionDenied`；全局 List → All；否则 `Scoped = nodeget_scoped_uuids ∩ operable_scoped_uuids`（必须同时具备 List 与某操作权限）。对 `NodeGet::ListAllAgentUuid` 检查加 `#[allow(deprecated)]`（该 variant 已弃用，新代码用 `MonitoringUuid::List`）。

## 数据库实体

| 表 | 列 | 约束 / 索引 / 关系 | 备注 |
|---|---|---|---|
| `monitoring_uuid` | `id`（auto pk，内存转 i16）、`uuid`（`Uuid`，UNIQUE）、`soft_delete`（bool） | UNIQUE(uuid) | 权威 Agent 注册表；`get_or_insert` 复活软删行；`soft_delete` 仅置标志。 |
| `static_monitoring` | `id`、`uuid_id`（i16，语义上指向 `monitoring_uuid.id`，无 DB 级 FK）、`timestamp`（i64 ms）、`storage_time`（Option<i64> ms）、`cpu_data`/`system_data`/`gpu_data`（JSON Value）、`data_hash`（`Vec<u8>`，16 字节） | 索引：`idx-static-uuid-timestamp`(`uuid_id`,`timestamp`)、唯一索引 `idx-static-uuid-data-hash`(`uuid_id`,`data_hash`)、`idx-static_monitoring-storage_time`(`storage_time`)；无实体 Relation | 8 列（SQLite 子批 999/8=124）；`report_static` 两级去重依赖唯一 `(uuid_id, data_hash)` 索引。 |
| `dynamic_monitoring` | `id`、`uuid_id`（i16，语义上指向 `monitoring_uuid.id`，无 DB 级 FK）、`timestamp`、`storage_time`、`cpu_data`/`ram_data`/`load_data`/`system_data`/`disk_data`/`network_data`/`gpu_data`（JSON Value） | 索引：`idx-dynamic-uuid-timestamp`(`uuid_id`,`timestamp`)、`idx-dynamic_monitoring-storage_time`(`storage_time`)；无实体 Relation | 11 列（SQLite 子批 999/11=90）；经 `MonitoringBuffer` 的 `dynamic_mon` 发送端插入。 |
| `dynamic_monitoring_summary` | `id`、`uuid_id`（i16，语义上指向 `monitoring_uuid.id`，无 DB 级 FK）、`timestamp`、`storage_time`、`cpu_usage`/`gpu_usage`（Option<i16>）、`used_swap`/`total_swap`/`used_memory`/`total_memory`/`available_memory`/`total_space`/`available_space`/`read_speed`/`write_speed`/`total_received`/`total_transmitted`/`transmit_speed`/`receive_speed`（Option<i64>）、`load_one`/`load_five`/`load_fifteen`（Option<i16> scaled）、`uptime`/`process_count`（Option<i32>）、`tcp_connections`/`udp_connections`（Option<i32>）、`boot_time`（Option<i64>） | 索引：`idx_dynamic_monitoring_summary_uuid_timestamp`(`uuid_id`,`timestamp`)、`idx-dynamic_monitoring_summary-storage_time`(`storage_time`)；无实体 Relation | 27 列（SQLite 子批 999/27=37）；`SCALED_SUMMARY_COLUMNS = cpu_usage/load_one/load_five/load_fifteen` 存为 ×10 的 i16；所有读路径经 `apply_descaling_to_json_object` `/10.0`。 |

迁移由 `ng-db` 维护（本 crate 不直接持有迁移）。

## Crate 内部约定

- **Feature gate**：`default = []` 仅暴露类型（`data_structure`、`query`）使 agent 可安全依赖；`server` feature 追加 `monitoring_buffer`、`monitoring_last_cache`、`monitoring_uuid_cache`、`static_hash_cache` 与 `rpc` 模块树。`lib.rs` 用 `#[cfg(feature = "server")]` gate 每个 server 模块。
- **jsonrpsee 约定**：自定义 jsonrpsee fork 用 `_`（非 `.`）作为命名空间分隔符（见 `#[rpc(server, namespace = "agent")]`、`"agent-uuid"`、`"nodeget-server"`）。仅使用 `#[rpc]` / `#[method]` proc 宏——**切勿**手写 `register_method`；mod.rs 中生成的 `*RpcServer` trait + `*RpcImpl` struct 经 `rpc_exec!` 委派到按文件分的自由函数。
- **返回类型**：所有 RPC 方法返回 `RpcResult<Box<RawValue>>`。成功响应通过 `RawValue::from_string` 或 `serde_json::value::to_raw_value` 手工拼装 JSON 数组/对象。report 函数中成功载荷缓存在 `static OnceLock<Box<RawValue>>`（如 `{"status":"buffered"}`），重复上报不重新分配。
- **Tracing target**：`"monitoring"`（agent 上报/查询/删除与多数 cache）、`"rpc"`（部分 list_all agent-uuid 入/出）、`"server"`（nodeget-server 与 agent-uuid span 名）、`"static_hash_cache"` 与 `"monitoring_uuid_cache"`（cache 专属事件）。
- **权限检查**：多数 RPC 用 `ng_token::get::check_token_limit`；`delete_dynamic` / `delete_dynamic_summary` 使用注入的 `PermissionChecker` trait（`require_permission_checker()` + `checker.check_token_limit`）。
- **数值转换**：所有 u64→i64/i32 用饱和 helper；f64→i16 截断（缩放函数）处显式抑制 cast lint。
- **列名/JSON 键**：查询路径用 `ng_core::utils::server_json::rename_and_fix_json` 重映射；summary 列 `column_name == json_key`。
- **注释语言**：全文中文文档/字段单位注释；编辑时保持一致。
- **Crate lint**：`#![warn(clippy::all, pedantic, nursery)]`，allow 了 `cast_sign_loss`/`cast_precision_loss`/`cast_possible_truncation`/`similar_names`/`dead_code`。

## 注意事项与陷阱

- **切勿对 summary 行重复反缩放**（`query.rs:308`）：`apply_descaling_to_json_object` **不是幂等的**，调用两次会除以 100。每条读路径必须恰好调用一次；`CachedEntry::new_summary`、过滤 multi-last 的 `descale_cached_summary`、DB 流式路径都假定单次调用。新增 summary 读路径若重复处理已反缩放值将静默损坏数据。
- **维护者必须同步缩放字段三处**（`data_structure.rs:347`）：新增 scaled 字段时，必须同时把列名加入 `query.rs:295` 的 `SCALED_SUMMARY_COLUMNS` 并满足 `DynamicSummaryQueryField::is_scaled`（由 `scaled_fields_match_single_source_of_truth` 测试强制）。`scale_cpu_percent_to_i16` 钳到 `[0,1000]`（0..=100.0%），`scale_load_to_i16` 钳到 i16 范围（允许 >100）。
- **切勿移除 static 两级去重的 DB 回退**（`rpc/agent/report_static.rs:108`）：`StaticHashCache` 每 `uuid_id` 只保留最新 hash；设备静态数据在中间变化后回退到旧值时，slow-path DB 检查仍能拦截。重启后 cache 重填可能漏掉旧重复，DB 查询是承重的。
- **维护者添加 delete 方法须有意选择权限路径**（`rpc/agent/delete_dynamic.rs:44`）：`delete_dynamic` 与 `delete_dynamic_summary` 用注入的 `PermissionChecker`（未注册则运行时失败），而 `delete_static` 直接用 `check_token_limit`。若注入 checker 未注册，dynamic/summary 删除会在运行时失败，static 删除却正常。
- **`query_dynamic`/`query_static`/`query_dynamic_summary` 的 `uuid_id_iter.next().unwrap()`**（`rpc/agent/query_dynamic.rs:152`）：在 condition fold 中使用，若权限/scopes 遍历与预解析遍历不一致会 panic。当前两遍循环以相同方式遍历同一 conditions 故安全，但**未来对 conditions 的重排序或过滤会触发 panic**（已标注「理论上不可能」）。
- **`rpc_module()` 的 `merge().expect()`**（`rpc/agent/mod.rs:67`）：跨 `agent` / `agent-uuid` / `nodeget-server` 命名空间的方法名重复会在 server 启动时 panic。`nodeget-server` 还被其他 crate 贡献方法，命名冲突是启动期风险。
- **`FLUSH_HANDLES` 的 `Mutex::unwrap()`**（`monitoring_buffer.rs:37`）：`init()` 与 `flush_and_shutdown()` 都 unwrap；前一持有者在持锁期间 panic 会使下次调用 panic（已文档化；实际持锁时间极短）。
- **`get_or_insert` 会复活软删行**（`monitoring_uuid_cache.rs:234`）：对软删行设置 `soft_delete=false` 并更新内存 map；`soft_delete` 仅置标志，行仍留在 `by_uuid`/`by_id`。`list_all()` 过滤软删，但 `get_id`/`get_uuid` 仍解析软删 UUID——通过 `get_id` 检查「存在」的代码会把软删 agent 视为存在。
- **`is_excluded_file_system` 大小写不敏感、其余大小写敏感**（`data_structure.rs:319`）：`is_virtual_interface` 与 `is_excluded_mount` 大小写敏感。匹配 `TMPFS` 文件系统是有意的；`br0` vs `BR0` 接口匹配则不是，实践中无碍。
- **`list_all_agent_uuid` 的弃用权限**（`rpc/nodeget/list_all_agent_uuid.rs:139`）：`NodeGet::ListAllAgentUuid` 检查被 `#[allow(deprecated)]` 包裹，该 variant 已弃用，新代码/token 应使用 `MonitoringUuid::List`；双重检查是过渡性的。
- **`get_dynamic_summary_last` 与 raw 路径的缩放不一致**（`monitoring_last_cache.rs:191`）：空字段返回完整 SCALED value（调用方须反缩放），而 multi-last raw 路径返回预反缩放的 serialized 形式。调用方必须知道走的是哪条路径；混用会产生 scaled 与 descaled 不一致。
- **`agent-uuid.delete` 文档/代码不一致**（`rpc/agent_uuid/delete.rs:31`）：trait 文档注释写「需 SuperToken 权限」，但实现仅检查 `MonitoringUuid::Delete + Scope::Global`——任何具该权限的 token 都能软删，并非只有 super-token。
- **`extract_limit_and_last` 的 10_000 钳制**（`rpc/agent/delete_common.rs:43`）：避免 limit/last 删除选出巨大 `Vec<i64>` id 列表导致 OOM。**切勿**在无内存上界的情况下移除或抬高此钳制。
- **`nodeget` 模块的 `rpc_exec!` 来源**（`rpc/nodeget/mod.rs:11`）：`use ng_db::rpc_exec` 而非 agent/agent-uuid 模块使用的 `ng_infra::rpc_exec`。两宏都存在；用错能编译但日志行为可能不同；重构时验证二者等价。

## 依赖关系

ng-monitoring 在 workspace 内依赖：`ng-core`（错误类型 `NodegetError`、`Scope`/`Permission`/`TokenOrAuth`、`utils::server_json::rename_and_fix_json`、`StaticDataQueryField`/`DynamicDataQueryField`）、`ng-db`（实体 `monitoring_uuid`/`static_monitoring`/`dynamic_monitoring`/`dynamic_monitoring_summary`、`load_from_db`、`ng_db::rpc_exec!`）、`ng-infra`（`make_global_cache!`、`DbBackedCache`、`RpcHelper`、`rpc_exec!`、`token_identity`）、`ng-token`（`get::check_token_limit`、token 解析）。server 二进制（`server/`）依赖 `ng-monitoring/server` 并在 `rpc_nodeget.rs::build_modules()` 调用 `ng_monitoring::rpc_module()` 把三命名空间合并进主 `RpcModule`；agent 二进制（`agent/`）依赖不带 `server` feature 的 `ng-monitoring`，仅复用 `data_structure` 与 `query` 类型来采集与上报。注入给 `delete_dynamic`/`delete_dynamic_summary` 的 `PermissionChecker` 由 server 二进制在 `serve.rs` 注册为 `ServerPermissionChecker`（最终委派到 `ng_token`）。
