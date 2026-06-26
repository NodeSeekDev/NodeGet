# nodeget-agent — 监控与自动化代理二进制

> 概览：`nodeget-agent` 是监控/自动化代理进程，以 WebSocket 客户端身份连接到一个或多个 NodeGet server，周期性采集静态/动态监控数据（CPU、RAM、磁盘、网络、GPU、进程）并通过 JSON-RPC 上报；同时接收并执行 server 下发的任务（ICMP/TCP/HTTP ping、HTTP 请求、DNS 查询、公网 IP 探测、命令执行、WebShell PTY、配置读/写、SelfUpdate），再把任务结果回传。支持配置热重载（由 EditConfig 任务或 SelfUpdate 成功后触发）、多 server 连接（指数退避重连）、启动时 NTP 校时，以及只采集不上报的 `--dry-run` 模式。

## 模块结构

```
agent/
├── build.rs                          # tier-3 glibc 目标的 getrandom 弱符号 shim 编译脚本
└── src/
    ├── main.rs                       # 进程入口：CLI 解析、配置加载/热重载、logger、NTP、spawn 4 个服务循环
    ├── config_access.rs              # 全局 AGENT_CONFIG RwLock 的统一只读访问器
    ├── dry_run.rs                    # --dry-run：采集一轮静态+动态数据并打日志，不联网
    ├── ntp.rs                        # 启动时获取 NTP 偏移（ms），失败回退本地时间
    ├── monitoring/
    │   ├── mod.rs                    # 全局 sysinfo/NVML 单例 + Instant 时间跟踪器
    │   ├── impls.rs                  # Monitor trait，组合 Static/DynamicMonitoringData，磁盘/网络速率计算
    │   ├── gpu.rs                    # NVML GPU 静态+动态采集（block_in_place 包裹 FFI）
    │   ├── system_impls/
    │   │   ├── mod.rs                # 系统静态/动态采集 + 5s 进程计数 ticker + 发行版精确版本识别
    │   │   ├── process.rs            # 跨平台进程计数（Windows EnumProcesses / Linux /proc）
    │   │   └── virtualization_detect.rs  # 虚拟化探测（vmaware / raw-cpuid）
    │   └── network_connections/
    │       ├── mod.rs                # 跨平台 (udp,tcp) 连接计数
    │       └── netlink.rs            # Linux SOCK_DIAG_BY_FAMILY 原始 netlink 实现
    ├── rpc/
    │   ├── mod.rs                    # JSON-RPC 请求构造（id=1）、入站 task/error 结构、error 消息处理循环
    │   ├── monitoring_data_report.rs # 静态/动态数据上报循环（热重载 + Arc<str> 共享序列化）
    │   └── multi_server.rs           # 多 server WebSocket 连接池 + connection_manager 重连
    └── tasks/
        ├── mod.rs                    # 任务分发：两级 JoinSet、TASK_POOL、WebShell 信号量、reload/SelfUpdate 触发
        ├── execute.rs                # 结构化命令执行（cmd+args，进程组超时清理，head/tail 截断）
        ├── dns.rs                    # hickory-resolver DNS 查询
        ├── http_request.rs           # HTTP 请求执行（method/headers/body/IP 族）
        ├── ip.rs                     # 公网 IP 探测（Cloudflare trace / ipinfo.io）
        ├── pty.rs                    # WebShell PTY 会话
        ├── self_update.rs            # 下载并替换 agent 二进制
        └── ping/
            ├── mod.rs                # 三种 ping 实现的入口
            ├── icmp.rs               # surge_ping ICMP ping
            ├── tcp.rs                # TCP 握手 ping
            └── http.rs               # HTTP GET ping
```

## 入口与启动流程

`main` (`agent/src/main.rs:106`)，签名 `async fn main() -> anyhow::Result<()>>`：

1. 安装 rustls aws-lc-rs provider（幂等，吞 AlreadyInstalled 错误）。
2. `AgentArgs::par()` 解析 CLI；处理 `--version`（打印后 `exit(0)`）。
3. 进入主 `loop`，每次迭代对应一次“配置加载 → 运行 → reload/退出”周期：
   - `parse_log_level` 把 `config.log_level: Option<String>` 转 `log::Level`，缺失或非法返回 `NodegetError::ParseError`（`main.rs:58`）。
   - logger 初始化：若已初始化则只 `set_max_level`，否则 `simple_logger::init_with_level`。
   - NTP 校时：仅在 `NTP_INIT_DONE` 为 `None` 时调用一次 `fetch_ntp_offset`，随后置 `true` —— reload 永不重新获取 NTP，避免时间跳变（`main.rs:148`）。
   - `update_global_config`：若 `AGENT_CONFIG` 已初始化则写替换内部 `Arc`，否则 `OnceLock::set`；RwLock 中毒 -> `NodegetError::Other`（`main.rs:74`）。
   - `dry_run()`：总是调用；非 `--dry-run` 为 no-op，`--dry-run` 则打日志后 `exit(0)`。
   - `init_connections`：为每个 server 创建 per-server 广播通道并 spawn `connection_manager`。
   - `init_process_count_ticker`：通过 `std::sync::OnceLock` 保证全进程仅启动一次。
   - spawn 4 个服务循环到 `handles: Vec<JoinHandle>`：`handle_static_monitoring_data_report`、`handle_dynamic_monitoring_data_report`、`handle_error_message`、`handle_task`。
   - `tokio::select! { biased; }` 在 `ctrl_c`（`abort_handles` 后 `break -> Ok(())`）与 `RELOAD_NOTIFY.notified()`（`abort_handles` 后继续 loop）之间选择。
4. `abort_handles` (`main.rs:91`) 抽干并 `.abort()` 每个 JoinHandle，用于 reload/退出前清理。

reload 触发点：`tasks/mod.rs` 中 EditConfig 成功后 `sleep 300ms` 再 `RELOAD_NOTIFY.notify_one()`；SelfUpdate 成功后 `sleep 300ms` 再触发平台重启（Windows `restart_process`，Unix `restart_process_with_exec_v`）。

## 公共 API

| 名称 | 签名 | 行为 |
|---|---|---|
| `main` (`main.rs:106`) | `async fn main() -> anyhow::Result<()>` | 入口；完整流程见上节 |
| `config_access::get_agent_config` (`config_access.rs:26`) | `pub fn get_agent_config() -> Result<Arc<AgentConfig>, NodegetError>` | Arc::clone 返回配置快照，O(1)；未初始化/RwLock 中毒 -> `NodegetError` |
| `config_access::current_agent_uuid` (`config_access.rs:41`) | `pub fn current_agent_uuid() -> uuid::Uuid` | 直接读 `agent_uuid`；未初始化/中毒 **panic**（不可恢复不变式） |
| `config_access::current_agent_uuid_string` (`config_access.rs:57`) | `pub fn current_agent_uuid_string() -> String` | 同上，返回 `.to_string()` |
| `dry_run::dry_run` (`dry_run.rs:18`) | `pub async fn dry_run()` | 采集一轮静态+动态数据并打日志；不做任何网络 I/O |
| `ntp::fetch_ntp_offset` (`ntp.rs:46`) | `pub async fn fetch_ntp_offset(ntp_server: &str) -> i64` | 解析 `server:123`、UDP `0.0.0.0:0` 发一次 NTP 请求（10s 超时）；偏移由 `(us/1000.0).round() as i64` 转 ms；任何失败返回 0 |
| `monitoring::init_process_count_ticker` (`monitoring/mod.rs:27`) | `pub fn init_process_count_ticker()` | 幂等；通过 std `OnceLock` 仅启动一次 5s 进程计数 ticker |
| `rpc::wrap_json_into_rpc_with_id_1` (`rpc/mod.rs:56`) | `pub fn wrap_json_into_rpc_with_id_1(method: &str, params: Vec<serde_json::Value>) -> String` | 构造 id=1 的 JSON-RPC 字符串；序列化失败时返回 `-32603` 错误字符串而非 panic |
| `rpc::monitoring_data_report::build_rpc_with_raw_data` (`rpc/monitoring_data_report.rs:64`) | `pub fn build_rpc_with_raw_data(method: &str, token: &str, data_json: &str) -> String` | 手工拼装 id=1 请求：token 走 serde 转义，`data_json` 原样嵌入（必须已是合法 JSON） |
| `rpc::monitoring_data_report::handle_static_monitoring_data_report` (`rpc/monitoring_data_report.rs:77`) | `pub async fn handle_static_monitoring_data_report()` | 每 `static_report_interval_ms`（默认 5min）上报静态数据 |
| `rpc::monitoring_data_report::handle_dynamic_monitoring_data_report` (`rpc/monitoring_data_report.rs:136`) | `pub async fn handle_dynamic_monitoring_data_report()` | 每 `summary_interval_ms`（默认 1s）上报 summary；每 `ticks_per_dynamic` tick 额外上报全量动态 |
| `rpc::handle_error_message` (`rpc/mod.rs:104`) | `pub async fn handle_error_message()` | 订阅各 server downlink，warning 形如 `result.error_id` ∈ 101..=999 的错误通知 |
| `rpc::multi_server::init_connections` (`rpc/multi_server.rs:68`) | `pub async fn init_connections(servers: Vec<Server>, connect_timeout: Duration) -> Vec<JoinHandle<()>>` | 为每个 server 建 broadcast(cap 32) + spawn manager；整体替换全局池 map |
| `rpc::multi_server::send_to` / `subscribe_to` (`rpc/multi_server.rs:556`) | `pub async fn send_to(server_name, Message) -> Result<()>; pub async fn subscribe_to(server_name) -> Result<broadcast::Receiver<Arc<Value>>>` | 上行发送 / 下行订阅；池未初始化/缺失/通道关闭 -> 错误 |
| `rpc::multi_server::build_connector` (`rpc/multi_server.rs:537`) | `pub fn build_connector(ignore_cert: bool) -> Option<Connector>` | `ignore_cert=false` 返回 `None`（webpki 默认根）；否则返回带 `NoCertificateVerification` 的 Rustls Connector |
| `tasks::handle_task` (`tasks/mod.rs:335`) | `pub async fn handle_task()` | 两级 JoinSet；过滤 `task_register_task`，`is_task_allowed` 门控，按池/信号量/`time::timeout(TASK_MAX_TIMEOUT)` 执行；结果经 `task_upload_task_result` 回传 |
| `tasks::execute::execute_command` (`tasks/execute.rs:37`) | `pub async fn execute_command(task: ExecuteTask) -> Result<String>` | 结构化 cmd+args，kill_on_drop + Unix process_group(0)，超时 SIGTERM→SIGKILL，head/tail 截断 |
| `tasks::dns::query_dns` (`tasks/dns.rs:39`) | `pub async fn query_dns(task: &DnsTask) -> Result<Vec<DnsRecordResult>, NodegetError>` | 支持 A/AAAA/TXT/PTR/MX/NS/SRV/SOA/CNAME/CAA |
| `tasks::http_request::execute_http_request` (`tasks/http_request.rs:74`) | `pub async fn execute_http_request(task: HttpRequestTask) -> Result<HttpRequestTaskResult>` | 30s 超时，body 流式读取硬上限 64MiB；非 ASCII header/body Base64 编码 |
| `tasks::ip::ip` (`tasks/ip.rs:50`) | `pub async fn ip() -> IPInfo` | 按 `ip_provider_or_default` 选 Cloudflare 或 IpInfo |
| `tasks::pty::handle_pty_url` (`tasks/pty.rs:105`) | `pub async fn handle_pty_url(url, terminal_id, ignore_cert) -> Result<()>` | 10s 连接超时；reserve terminal_id 后运行 PTY 会话 |
| `tasks::self_update::self_update` (`tasks/self_update.rs:20`) | `pub async fn self_update(tag: &str) -> bool` | 失败一律返回 `false`（任务级错误），成功返回 `true` 由调用方触发重启 |

## 关键类型与常量

### 全局单例（`main.rs:45`）

| 名称 | 类型 | 说明 |
|---|---|---|
| `AGENT_ARGS` | `OnceLock<AgentArgs>` | 启动一次设置 |
| `AGENT_CONFIG` | `OnceLock<RwLock<Arc<AgentConfig>>>` | O(1) Arc-clone 读；reload 时写替换内部 Arc |
| `RELOAD_NOTIFY` | `LazyLock<tokio::sync::Notify>` | 热重载信号（EditConfig / SelfUpdate 通知） |
| `NTP_INIT_DONE` | `OnceLock<bool>` | 阻止 reload 重复获取 NTP |

### 监控全局（`monitoring/mod.rs`）

- `GLOBAL_SYSTEM / GLOBAL_DISK / GLOBAL_NETWORK / GLOBAL_GPU` (`mod.rs:32`)：`OnceCell<Mutex<…>>` 单例（System/Disks/Networks 与 `Option<Nvml>`，NVML 在无驱动时为 `None`）。
- `DISK_TIME_TRACKER / NETWORK_TIME_TRACKER` (`mod.rs:62`)：`Mutex<Instant>` 速率分母；首次初始化时回拨 1s（`checked_sub`，回退 `now`），保证首 tick 间隔非零。
- `Monitor` trait (`impls.rs:21`)：`async fn refresh_and_get() -> Self`；分别为 `StaticMonitoringData` (`impls.rs:46`)、`DynamicMonitoringData` (`impls.rs:77`) 实现。

### 进程计数（`system_impls/mod.rs`）

- `PROCESS_REFRESH_INTERVAL_SECS` (`mod.rs:24`)：`5` 秒。
- `PROCESS_COUNT_CACHE` (`mod.rs:32`)：`AtomicU32`（**非** U64 —— 部分 32 位 tier-3 目标无 AtomicU64）；`u32::MAX` 远大于真实计数。
- `PROCESS_TICKER_STARTED` (`mod.rs:35`)：`std::sync::OnceLock`，跨 reload 仅启动一次。
- `PROCESS_TICKER_STARTED` + `tokio::interval(5s)` 使用 `MissedTickBehavior::Delay`，并消费首个 immediate tick。

### 网络 netlink（`monitoring/network_connections/netlink.rs`）

- `SOCK_DIAG_BY_FAMILY` (`netlink.rs:18`)：`20`；`ALL_TCP_STATES` = `0xffffffff` 再收窄为 `1<<1`（`TCP_ESTABLISHED`）；UDP 查询所有 state。
- `RECV_BUF` (`netlink.rs:42`)：`thread_local RefCell<Vec<u8>>`，每秒 4 次（tcp4/tcp6/udp4/udp6）复用，按 page_size 重置。
- `InetDiagSockId / InetDiagReqV2` (`netlink.rs:60`)：`#[repr(C)]` 镜像 `linux/inet_diag.h`。
- `FdGuard` (`netlink.rs:311`)：raw netlink fd 的 RAII，`Drop` 调 `libc::close`。

### 多 server 连接池（`rpc/multi_server.rs`）

- `ServerHandle` (`multi_server.rs:36`)：持有 `uplink_tx: broadcast::Sender<Message>`（agent→server）与 `downlink_tx: broadcast::Sender<Arc<serde_json::Value>>`（server→agent，已预解析）。
- `CONNECTION_POOL` (`multi_server.rs:44`)：`static OnceCell<RwLock<HashMap<String, Arc<ServerHandle>>>>`，key 为 `server.name`；reload 时整体替换。
- `UuidVerification` (`multi_server.rs:369`)：`Ok / Transport(String) / Mismatch{expected,got}` —— Mismatch 触发 30s 冷却。
- 重连退避：`connect_with_retry` (`multi_server.rs:435`)，`base*2^(retry-1)`，`MAX_BACKOFF=60s`、`BASE_BACKOFF=1s`、`±20%` jitter（`rand 0.8..1.2`），retry 计数先 saturating-sub 16 再移位；无重试上限。已建立连接断开后固定 sleep **3s** 再重连。
- `NoCertificateVerification` (`multi_server.rs:493`)：rustls `ServerCertVerifier`，全部 `verify_*` 返回 `assertion()`；仅在 `ignore_cert=true` 时启用。

### 任务池与信号量（`tasks/mod.rs`）

| 常量 | 值 | 说明 |
|---|---|---|
| `TASK_MAX_TIMEOUT` (`tasks/mod.rs:40`) | 10 分钟 | 任何非 WebShell 任务在 per-message JoinSet 槽的硬上限 |
| `TASK_POOL_MAX_CONCURRENCY` (`tasks/mod.rs:43`) | 10 | 网络任务并发许可 |
| `TASK_POOL_PER_TASK_TIMEOUT` (`tasks/mod.rs:43`) | 10s | 单任务硬超时（覆盖子任务自带超时） |
| `TASK_POOL` (`tasks/mod.rs:43`) | `Semaphore(10)` | Ping/TcpPing/HttpPing/HttpRequest/Ip/Dns 走池；WebShell/Execute/ReadConfig/EditConfig/Version/SelfUpdate 不走池（`is_pool_managed` `tasks/mod.rs:119` const fn） |
| `WEBSHELL_MAX_SESSIONS` (`tasks/mod.rs:108`) | 8 | WebShell 同时会话上限 |
| `WEBSHELL_SESSION_SEMAPHORE` (`tasks/mod.rs:108`) | `Semaphore(8)` | `try_acquire`，无排队；满即立即失败，且**绕过** `TASK_MAX_TIMEOUT` |

### 命令执行（`tasks/execute.rs`）

- `EXECUTE_TIMEOUT` (`execute.rs:18`)：1 分钟；`GRACE_AFTER_SIGTERM`（Unix）SIGKILL 前的 SIGTERM 宽限。
- `read_capped` (`execute.rs:202`)：读到 `max_capture` 字节后继续 copy 到 `tokio::io::sink()` 排空管道；否则 OS 管道缓冲（64KiB）被写满会阻塞子进程 `write()`，使 `child.wait()` 永不返回。

### HTTP / IP / Ping 常量

- `HTTP_REQUEST_TIMEOUT=30s`、`HTTP_RESPONSE_MAX_BYTES=64MiB`（`http_request.rs:33`）；`IP_BOUND_CLIENTS` (>32 清空，`http_request.rs:26`)。
- `CLIENT_V4 / CLIENT_V6` (`ip.rs:32`)：每族一个 `tokio OnceCell<Client>`，绑定 UNSPECIFIED，5s 超时；`IPInfo{ipv4,ipv6}` (`ip.rs:39`)。
- `ping/http.rs:12`：`GLOBAL_CLIENT`（OnceCell）、`PING_TIMEOUT=10s`。
- `ping/icmp.rs:17`：`ICMP_PAYLOAD`（8 字节 0）、`PING_TIMEOUT=2s`（含目标 DNS 解析）；`GLOBAL_ICMP_V4_CLIENT / GLOBAL_ICMP_V6_CLIENT` (`:23`)。
- `ping/tcp.rs:13`：`PING_TIMEOUT=1s`（与 TCP 系统重传对齐，**勿改**）。

### PTY（`tasks/pty.rs`）

- `TERMINAL_CONNECTION_POOL` (`pty.rs:29`)：`LazyLock<Arc<RwLock<HashSet<String>>>>` 跟踪活动 `terminal_id`；中毒经 `into_inner` 恢复。
- `TerminalIdGuard` (`pty.rs:61`)：reserve/release RAII，防止重复连接。
- `NeedResize / HeartBeat` (`pty.rs:399`)：`#[serde(rename="type")]` 的 resize/heartbeat 载荷结构。
- `parse_url` (`pty.rs:493`)：校验 ws/wss；`/auto_gen` 时重建为 `scheme://host:port/terminal?agent_uuid&task_id&task_token&terminal_id`，并始终 set/replace `terminal_id` 查询参数。

### NTP（`ntp.rs`）

- `DEFAULT_NTP_PORT=123`、`NTP_TIMEOUT=10s` (`ntp.rs:14`)。
- `StdTimestampGen` (`ntp.rs:20`)：基于 `SystemTime`，`init()` 捕获距 UNIX_EPOCH 的 duration（错误 -> 0）。
- 偏移转换 `(us/1000.0).round() as i64`（**非** 整除，见陷阱）。

### 构建（`agent/build.rs`）

- `needs_getrandom_shim` (`build.rs:16`)：对 `mips*-linux-gnu`、`armv5te-linux-gnu`、`powerpc-unknown-linux-gnu` 返回 true —— 这些 tier-3 目标的交叉 Docker 镜像可能带 glibc < 2.25（无 `getrandom` 符号）。

## 内部机制

### 启动/重载生命周期

`main.rs:130-194` 的单个 `loop` 每轮重新读取配置：`parse_log_level` → logger（已初始化则只调 `set_max_level`）→ 仅当 `NTP_INIT_DONE is None` 时获取 NTP（置 true）→ `update_global_config` → `dry_run()`（非 dry-run 为 no-op）→ `init_connections` → `init_process_count_ticker` → spawn 4 个服务循环 → `tokio::select!{biased; ctrl_c, RELOAD_NOTIFY.notified()}`。reload 由 `tasks/mod.rs:563-569` 的 EditConfig-success 或 SelfUpdate-success 路径触发。

### 全局配置单例

`AGENT_CONFIG = OnceLock<RwLock<Arc<AgentConfig>>>` (`main.rs:47`)。`update_global_config` 写新 Arc 或 set OnceLock；所有读经 `config_access::get_agent_config()`（读锁 + Arc::clone，O(1)）；热路径 uuid 访问器 panic-on-missing（无可恢复不变式）。每个上报 tick 都重新读一次配置，立即拾取 server 列表/token/interval 变化。

### 监控采集速率分母

`monitoring/mod.rs:84-116`（磁盘）、`144-167`（网络）：Instant 跟踪器首次初始化回拨 1s，保证首 tick 间隔非零；`impls.rs:145,194` 额外把分母 clamp 到 `>=0.01s`（10ms），防 clock 异常产生 `inf`/`u64::MAX`。读写/收发速率 = `(bytes as f64 / safe_interval_secs) as u64`。

### 监控四路并发

`impls.rs:77-127`：`DynamicMonitoringData::refresh_and_get` 在一个父任务内 `tokio::join!` 四个 future（system / gpu NVML block_in_place / disk / network，其中 network 内部 `spawn_blocking(calc_connections)`）。panic 直接传播（无 JoinError 回退）。静态为 system+gpu 两路 join（`impls.rs:55`）。

### 进程计数 ticker 解耦

`system_impls/mod.rs:40-65`：`std::sync::OnceLock` 保证跨 reload 仅启动一次。立即 refresh 一次，随后 `tokio::interval(5s, MissedTickBehavior::Delay)`（消费首个 immediate tick）。`refresh_process_count` 经 `spawn_blocking(count_processes)` 执行并以 `Relaxed` 存入 `AtomicU32`；动态 tick（1s）仅读 `cached_process_count()`（u64 load），**绝不**在热路径枚举 `/proc`。

### 网络连接计数路径

`impls.rs:214-217` 调 `spawn_blocking(calc_connections)`。Linux：`netlink.rs` 每次调用打开 `AF_NETLINK/NETLINK_SOCK_DIAG` raw socket，发送序列化的 `inet_diag_req_v2`（TCP 收窄 `1<<1` ESTABLISHED，UDP 全 state），recvfrom 进 thread-local page-sized 缓冲 `RECV_BUF`，统计非控制 nlmsg 记录，遇 `NLMSG_DONE/ERROR` 停止；`FdGuard` Drop 关闭 fd；header 读取用 `ptr::read_unaligned` 以避免在严格对齐目标（如 ARMv7）上 SIGBUS。Windows：`netstat2 iterate_sockets_info_without_pids` 折叠。其他平台：常量 `(0,0)`。

### 多 server 连接池

`multi_server.rs`。`CONNECTION_POOL = OnceCell<RwLock<HashMap<String, Arc<ServerHandle>>>>`。`init_connections` 为每个 server 创建 uplink（`broadcast Message`，cap 32）+ downlink（`broadcast Arc<Value>`，cap 32）通道，spawn 一个 `connection_manager`，然后通过 `mem::replace` + drop 旧 map 整体替换池（及时释放旧 sender）。每个 `connection_manager` 是外层重连循环：`connect_with_retry`（指数 1s..60s ±20% jitter）→ `verify_server_uuid`（Ok/Transport/Mismatch）→ 若 `allow_task` 发 `task_register_task` 并校验 ack（id==1、无 error、有 result）→ 内层 `select!`：`uplink_rx.recv()` → `ws_write.send()`，`ws_read.next()` 的 Text 帧（一次性解析为 `Arc<Value>`）→ `downlink_tx`，外加 1 分钟 `task_resubscribe`；任意中断后 sleep 固定 3s 再重连。`send_to` / `subscribe_to` 为上报与任务循环使用的上行/下行接口。

### 上报循环与序列化共享

`monitoring_data_report.rs`。`handle_static_monitoring_data_report` tick `static_report_interval_ms`（默认 5min，`MissedTickBehavior::Skip`，消费首 tick）；`handle_dynamic_monitoring_data_report` tick `summary_interval_ms`（默认 1s），每 `ticks_per_dynamic`（= `dynamic_interval_ms/summary_interval_ms`）额外发全量 dynamic。两者每 tick 重读 `AGENT_CONFIG`；interval 变化时重建 ticker（消费 immediate tick 以防双发），dynamic 还重置 `tick_count`。载荷经 `serialize_shared` 一次序列化为单个 `Arc<str>`，跨 per-server spawn 任务以 `Arc::clone` 共享；`build_rpc_with_raw_data` 手工拼装 JSON-RPC 字符串（token serde 转义、预校验的 `data_json` 原样嵌入），绕开 `serde_json::Value` 物化。序列化失败时 `continue` 跳过本轮（绝不发 null 误导 server）。

### 任务分发与并发

`tasks/mod.rs`。`handle_task`（两级 JoinSet）：per-server 外层任务订阅 downlink；per-message 内层任务。每条消息 `from_value` 成 `JsonRpcTask`，过滤 `method == 'task_register_task'`（TODO：拆成 `task_subscribe`/`task_dispatch`），`is_task_allowed` 门控，再执行。网络任务（Ping/TcpPing/HttpPing/HttpRequest/Ip/Dns）走 `TASK_POOL`（Semaphore 10，单任务硬超时 10s，排队等待不计入超时）；WebShell 用 `WEBSHELL_SESSION_SEMAPHORE` `try_acquire`（上限 8，无排队，满即失败）并绕过 `TASK_MAX_TIMEOUT`（10min）—— 它是长会话；其他任务统一 `time::timeout(TASK_MAX_TIMEOUT)`。结果包装成 `TaskEventResponse` 经 `task_upload_task_result` 发送；序列化失败有三级回退 ack 防止 server 永挂。EditConfig 成功 → `sleep 300ms` → `RELOAD_NOTIFY.notify_one()`；SelfUpdate 成功 → `sleep 300ms` → 平台重启（Windows `restart_process`，否则 `restart_process_with_exec_v`）。

### Execute 任务输出处理

`execute.rs`。结构化 cmd+args（无 shell）。`kill_on_drop(true)` + Unix `process_group(0)` 使子进程 pgid == 其 pid。`read_capped` 读到 `exec_max_character` 后继续 copy 到 `tokio::io::sink()` —— 这段排空是 load-bearing：否则 OS 管道缓冲（64KiB）被写满，子进程 `write()` 阻塞，`child.wait()` 永不返回，任务只能等到 `EXECUTE_TIMEOUT`（1min）才结束且截断逻辑失效。超时（Unix）：`libc::killpg(pgid, SIGTERM)` → 等 `GRACE_AFTER_SIGTERM`（2s）→ `start_kill`。截断：head=max_chars/2、tail=max_chars-head，各自向内 clamp 到 UTF-8 `char_boundary`；相遇则直接截断，否则用 `'[... truncated ...]'` 拼接。

### WebShell PTY 流水线

`pty.rs`。`handle_pty_url` reserve `terminal_id`（全局 `RwLock<HashSet>` + `TerminalIdGuard` RAII），10s 连接 WS，解析 shell，运行 `handle_pty_session`。会话：`openpty(24x80)`；设置 `TERM/LANG/LC_ALL` 与显式 `PATH/HOME/USER`（非 Windows）；`spawn_blocking` 读线程以 `try_send` 填 `mpsc(4096)`（满即丢弃，避免阻塞读线程）；一个 tokio 任务把 channel 排空到 WS sender；另一任务读 WS 派发 resize（`pair.master.resize`）/ heartbeat（drop）/ 终端输入（`write_all`）。`select!{biased}`：先结束的一侧 abort 另一侧（abort `pty_to_ws_task` 会 drop channel，使 spawn_blocking 读线程下次 send 退出）。关闭：Unix `killpg(SIGTERM)` → 200ms → `killpg(SIGKILL)` → `child.wait()`；非 Unix `child.kill()`。

### rustls provider 初始化策略

`main.rs:111` 启动时安装一次 aws-lc-rs `default_provider`，吞 `AlreadyInstalled` 错误（第三方依赖先安装不会 panic）。同样的幂等模式（`OnceLock`/`Once` + `let _ = install`）在 `http_request.rs:43`、`ping/http.rs:22`、`ip.rs:67` 惰性重复执行 —— reqwest 配的是 `rustls-no-provider`、tokio-tungstenite 配的是 `rustls-tls-webpki-roots`，任何 TLS 握手前必须安装 provider。

### build.rs getrandom shim

`agent/build.rs`：对 tier-3 glibc Linux 目标（`mips*-linux-gnu`、`armv5te-linux-gnu`、`powerpc-unknown-linux-gnu`，交叉镜像可能带 glibc <2.25）编译 `getrandom_shim.c` 为静态库，提供弱符号 `getrandom()`；若 glibc 已提供则弱符号被无害覆盖。

## RPC 方法

Agent 作为客户端调用/接收的 JSON-RPC（方法名由 server 定义，jsonrpsee fork 分隔符为 `_`）。Agent 侧手工拼装 JSON-RPC，无 jsonrpsee client。

| 命名空间 | 方法 | 参数 | 所需权限 | 行为 |
|---|---|---|---|---|
| `task` | `task_register_task`（agent→server，订阅） | `[token:string, agent_uuid:string]` | `server.allow_task==true` + 每 server token | WS 建连后发一次（若 allow_task）且每 1 分钟重发；agent 校验 ack（id==1、无 error、有 result），不符则重连 |
| `task` | `task_register_task`（server→agent，下发） | `JsonRpcTask{method, params:{result:TaskEvent}}` | `is_task_allowed` 门控 | `tasks/mod.rs` 过滤同名方法，解析 `task_event_type`，校验后执行并经 `task_upload_task_result` 回传。类型：Ping/TcpPing/HttpPing/HttpRequest/WebShell/Execute/ReadConfig/EditConfig/Ip/Dns/Version/SelfUpdate |
| `task` | `task_upload_task_result` | `[token, TaskEventResponse]` | 每 server token | 每个任务完成后调用；三级回退保证总有 ack |
| `nodeget-server` | `nodeget-server_uuid` | `[]` | 无（预认证握手） | `verify_server_uuid` 发送，5s 等 Text 帧 result 字符串 uuid；不匹配 30s 冷却，传输错误指数重连 |
| `agent` | `agent_report_static` | `[token, StaticMonitoringData]` | 每 server token | 静态上报，每 `static_report_interval_ms`（默认 5min） |
| `agent` | `agent_report_dynamic_summary` | `[token, DynamicMonitoringSummaryData]` | 每 server token | 动态 summary，每 `summary_interval_ms`（默认 1s），按 select_disk/select_network_interface 过滤 |
| `agent` | `agent_report_dynamic` | `[token, DynamicMonitoringData]` | 每 server token | 全量动态，每 `dynamic_interval_ms`（默认 1s，即默认每 tick） |

鉴权流程：每 server 持有 config 中的独立 token，所有上报/任务方法的第一参数都是该 token。握手阶段先 `nodeget-server_uuid` 校验身份，再 `task_register_task` 订阅任务流；server 下发任务与订阅共用同一方法名（见陷阱）。

## Crate 内部约定

- **Edition 2024**，二进制名 `nodeget-agent`；依赖仅 `ng-core/for-agent`、`ng-config`、`ng-task`、`ng-monitoring`，**绝不**依赖 `ng-db`/`ng-infra`/任何 server-only crate；所有业务 crate 的 `server` feature 关闭。
- async 单例统一用 `tokio::sync::OnceCell::const_new()`；std 单例用 `OnceLock`/`LazyLock`。热路径缓存频繁以 `Arc<...>` 快照返回，调用方 Arc clone 而非深拷贝。
- **rustls aws-lc-rs provider**：`main.rs` 安装一次并在 `http_request.rs`、`ping/http.rs`、`ip.rs` 经 `OnceLock` helper 惰性重装（吞错），因为 reqwest 用 `rustls-no-provider`。
- 共享全局状态位于 crate 根（`main.rs`）：`AGENT_ARGS`、`AGENT_CONFIG`、`RELOAD_NOTIFY`、`NTP_INIT_DONE`；其他模块以 `crate::AGENT_CONFIG` 等访问。
- 热路径配置读统一走 `crate::config_access::get_agent_config()`（返回 `Arc<AgentConfig>`，O(1) Arc clone），绝不深拷贝；仅取 uuid 的热路径用 `current_agent_uuid[_string]()` panic-on-missing 辅助器。
- 所有后台循环（`handle_error_message`、`handle_task`、静态/动态上报）启动时 `sleep 1s`，再持有外层 JoinSet（生命周期=函数）；reload 时主循环 abort 顶层 handle，JoinSet drop 级联到所有子任务。
- 错误：内部以 `NodegetError`（ng-core）冒泡；`anyhow` 仅作顶层 main 返回与 `tasks::Result`；子模块各自 `pub type Result<T> = std::result::Result<T, NodegetError>` 或 `anyhow::Result<T>`。
- 日志用 `log` crate + `simple_logger`；中英混排。Logging targets：`"monitoring"`（`netlink.rs`）、`"task"`（`http_request.rs`、`ping/http.rs`）。
- JSON-RPC 字符串部分手工拼装（agent 侧无 jsonrpsee client）：`wrap_json_into_rpc_with_id_1(method, params)` 固定 id=1；`build_rpc_with_raw_data(method, token, data_json)` 在热上报路径绕开 `serde_json::Value` 物化。
- 上游 RPC 方法名：`nodeget-server_uuid`、`task_register_task`、`task_upload_task_result`、`agent_report_static`、`agent_report_dynamic`、`agent_report_dynamic_summary`。
- **tier-3 Linux 交叉编译**：`build.rs:16` 为 mips/armv5te/powerpc gnu 目标编译 getrandom() 弱符号 shim。
- crate 根自定义 lint（`main.rs:8`）：`#![warn(clippy::all, pedantic, nursery)]` 加精选 allow-list（`cast_sign_loss/precision_loss/possible_truncation`、`similar_names`、`too_many_lines`、`significant_drop_tightening`、`dead_code`）。
- **平台条件编译**：`netlink.rs`/`process.rs` 的 Linux 路径 `#[cfg(target_os = "linux")]`；netstat2/EnumProcesses/raw-cpuid 路径 `#[cfg(target_os = "windows")]`；macOS（及其他）路径返回 0/TODO stub。macOS 当前不支持进程计数与连接计数。
- sysinfo feature 按平台 gating：非 Windows 启用 `multithread/disk/network/system`；Windows 额外加 `windows` feature 并引入 netstat2 + raw-cpuid + windows-sys。

## 注意事项与陷阱

- **维护者切勿**把 `task_register_task` 当作单纯订阅方法 —— 它同时被 server 用来下发任务（`tasks/mod.rs:419` 过滤的就是这个字符串）。任何协议变更必须兼顾两种语义；改错会静默丢弃所有任务下发。代码有 TODO 拆分为 `task_subscribe`/`task_dispatch`。
- **维护者必须**在再次调用 `init_connections` 前 abort 之前的 handles（`multi_server.rs:68`）。`main.rs` 通过 `abort_handles` 履约；违反契约会让旧 manager 短暂存活并重连。池 replace + drop-old-map 仅是纵深防御。
- `subscribe_to` (`multi_server.rs:600`) 返回的 `broadcast::Receiver` 只能看到订阅**之后**广播的消息；调用方必须容忍 `RecvError::Lagged`（warn+continue），且若 manager 尚未建连，receiver 可能无限空闲（需自带超时）。
- WebShell 会话**故意**绕过 `TASK_MAX_TIMEOUT`（`tasks/mod.rs:437`），仅由 `WEBSHELL_SESSION_SEMAPHORE`（8 并发、`try_acquire` 非阻塞）约束。恶意/异常 server 发 >8 个 WebShell 会立即失败，但 8 个并发 PTY 子进程仍会跑到 WS 关闭。**切勿**把 WebShell 包进 `time::timeout`。
- `read_capped` (`execute.rs:84`) 命中 `max_capture` 后向 `sink()` 的持续排空是 load-bearing；移除会让 OS 管道缓冲（64KiB）写满、子进程 `write()` 阻塞、`child.wait()` 永不返回，导致 60s 才超时且 head/tail 截断逻辑失效。该 60s 排空由 `EXECUTE_TIMEOUT` 兜底，并非无界。
- `exec_max_character` (`execute.rs:43`) 名为 “character”，但比较/截断按 UTF-8 **字节**长度（`result.len()`）。多字节语言（中文 3B/char）实际字符数少于名字暗示；`is_char_boundary` 保证字符串合法。**切勿**假设按字符计数。
- 磁盘/网络速率分母 clamp 到 `>=0.01s`（10ms）且首初始化回拨 1s（`impls.rs:145`）。动速率计算的人**必须**保留 clamp 与回拨，否则首 tick 速率会变成垃圾。
- **切勿**把 `PROCESS_COUNT_CACHE` 从 `AtomicU32` “升级”为 `AtomicU64`（`system_impls/mod.rs:32`）—— 部分 32 位 tier-3 目标（armv5te/mipsel/powerpc/thumbv6m）没有 `AtomicU64`；`DynamicSystemData.process_count` 在读取时 u64 拓宽。
- `DynamicDataFromGpu::new()` (`gpu.rs:90`) 用 `filter_map` 要求每个 NVML 字段都成功 —— 单字段失败会整卡丢弃；`update()`（热路径）改用字段级 `Ok(...)`，绝不丢弃已存在的 GPU，支持 vGPU 热插拔。两条路径的容错语义不同，**切勿**统一。
- server UUID 不匹配触发 **30s 冷却**（`multi_server.rs:159`），而非传输错误的指数退避。这是故意的（通常意味着 URL/DNS/反代配错，快速重连只是噪声）。**切勿**改成快速重试。
- `NoCertificateVerification` (`multi_server.rs:493`) 仅在 `server.ignore_cert=true` 时接入，接受任意证书。**生产环境切勿**开启 `ignore_cert`，**切勿**在别处复用该 verifier 模式。
- `ip_cloudflare` (`ip.rs:174`) 用 IP 字面量 URL（`1.1.1.1`、`[2606:4700:4700::1111]`）以确保 DNS 解析族与 `local_address` 绑定一致；换成 `www.cloudflare.com` 会让 DNS 选错族并破坏 v4/v6 之一。这些 anycast IP 的证书带 IP SAN。
- TCP ping 超时硬编码 **1s**，与 TCP 系统重传对齐（`ping/tcp.rs:13`）。改它会改变测量语义（开始计入重传）。代码注释明确写**勿改**。
- `RESOLVER_CACHE` (`dns.rs:28`) 对进程生命周期缓存 system-conf resolver，`/etc/resolv.conf` 改动要重启才生效；缓存按已解析 `SocketAddr` 去重（`1.1.1.1` 与 `1.1.1.1:53` 共享一条）。已知取舍。
- `TaskEventResponse` 序列化有三级回退（完整 → 最小错误 ack → 手工 `json!()`）（`tasks/mod.rs:519`），确保序列化失败也不会让任务在 server 侧永久 pending。任何重构**必须**保留 always-ack 属性。
- `self_update` (`self_update.rs:26`) 在所有失败路径（含 `canonical_exe_path` 为 None）返回 `false`。过去曾 `exit(1)`，导致一次坏任务杀掉整个 agent。**任务中绝不**终止进程，必须返回 false 当作普通任务错误上报。
- NTP 偏移由 `(us/1000.0).round() as i64` 转 ms（`ntp.rs:72`），**非**整除 —— 小负偏移整除会向零截断并扭曲 ±几十 us 的 LAN 偏移。**保留**四舍五入。
- NTP 每进程仅获取一次（`NTP_INIT_DONE` 守卫，`main.rs:148`）。reload **故意**不重新获取以避免偏移跳变；新增重取路径必须谨慎 gating。
- 上报循环序列化失败时**跳过本轮**（`continue`）而非发 null（`monitoring_data_report.rs:107`）—— 发 null 会欺骗 server、掩盖真实失败。**保留** skip-on-serialize-fail。
- Unix 子进程以 `process_group(0)` 启动（`execute.rs:59`），pgid == pid，使超时时 `libc::killpg(pgid, ...)` 能回收孙进程（如 shell fork 的子进程）。移除 `process_group(0)` 会让孙进程在超时后变孤儿；非 Unix 回退 `kill_on_drop`。

## 依赖关系

`nodeget-agent` 仅依赖 4 个 workspace crate：`ng-core`（启用 `for-agent` feature）、`ng-config`、`ng-task`（默认 feature，仅类型/数据结构）、`ng-monitoring`（默认 feature，仅监控数据结构与缓存类型）。它**不**依赖 `ng-db`/`ng-infra`/`ng-token`/`ng-kv`/`ng-crontab`/`ng-js-runtime`/`ng-js-worker`/`ng-static`/`ng-terminal`，也不开启任何业务 crate 的 `server` feature —— 这保证了 agent 二进制无 server-side RPC handler、无 DB 耦合。它是 workspace 的叶子消费方，没有 workspace crate 反向依赖它。主要外部依赖：`tokio`、`reqwest`（rustls-no-provider）、`tokio-tungstenite`（rustls-tls-webpki-roots）、`sysinfo`、`nvml-wrapper`、`surge_ping`、`hickory-resolver`、`vmaware`、`sntpc`、`simple_logger`、`libc`。
