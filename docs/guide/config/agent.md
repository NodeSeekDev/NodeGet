# Agent 配置

官方脚本安装的 `nodeget-agent` 的配置路径位于 `/etc/nodeget-agent.conf`

```toml
# 日志等级，必填，可选 trace / debug / info / warn / error
# 未设置时 Agent 启动会报错退出
# 如果你正在测试或遇到问题，请至少选择 debug
log_level = "info"

# 动态监控数据上报间隔（毫秒），默认 1000
dynamic_report_interval_ms = 1000

# 动态监控摘要数据上报间隔（毫秒），默认 1000
# 必须是 dynamic_report_interval_ms 的因数
# 即 dynamic_report_interval_ms 必须是该值的整数倍
# 例如 dynamic_report_interval_ms = 4000, dynamic_summary_report_interval_ms = 2000
# 则每 2 秒上报一次摘要，每 4 秒上报一次完整动态数据
# dynamic_summary_report_interval_ms = 1000

# 静态监控数据上报间隔（毫秒），默认 300000（5 分钟）
# static_report_interval_ms = 300000

# Agent 的 Uuid，建议设置为 auto_gen，首次启动时会随机生成并持久化到配置文件
# 如果不是 auto_gen，请自行确保每个 Agent 的 uuid 唯一，否则可能导致数据混乱或 UB
agent_uuid = "auto_gen"

# 连接超时时间（毫秒），默认 1000
connect_timeout_ms = 1000

# 终端 Shell，Linux/macOS 下默认 `/bin/bash`（不存在时 fallback 至 `sh`），Windows 下默认 `cmd.exe`
terminal_shell = "bash"

# 执行命令输出的最大字符数量限制
# 超出该数量只返回命令的最后结果，上文将被截断，默认 10000
exec_max_character = 10000

# IP 地址获取服务提供商，可选 ipinfo / cloudflare，默认 cloudflare
ip_provider = "cloudflare"

# NTP 服务器地址，用于获取本地时间与 NTP 参考时间的偏差
# Agent 仅在首次启动时查询该服务器，所有时间戳输出自动应用此偏移
# 连接失败或超时时自动降级为本地时间（偏移为 0），不影响业务
# 默认使用 pool.ntp.org
ntp_server = "pool.ntp.org"

# Disk 选择列表（按 mount_point 匹配），用于 Dynamic Summary 上报
# 若指定且非空，则仅统计列表中的磁盘；否则回退到默认排除逻辑
# 默认排除规则会按挂载点前缀自动过滤掉虚拟/临时挂载点
# dynamic_summary_select_disk = ["/", "/data"]

# 网卡选择列表（按 interface_name 匹配），用于 Dynamic Summary 上报
# 若指定且非空，则仅统计列表中的网卡；否则回退到默认排除逻辑
# 默认排除规则会自动过滤掉 br、cni、docker、podman、flannel、lo、veth、virbr、vmbr、tap、fwbr、fwpr 等前缀的虚拟网卡
# dynamic_summary_select_network_interface = ["eth0", "eth1"]

# 服务器列表
# 可指定多个，以连接多个 Servers
[[server]]

# Server 名称
# 必须指定，用于展示与内部判断，可自由命名
name = "test_server1"

# Server UUID
# 必须指定，用于连接时校验服务器身份
# Agent 连接成功后会调用 nodeget-server_uuid 获取远端 UUID 并与此值比对
# 不匹配时打印 error 日志并跳过该 Server（不影响其他 Server）
# 可通过 nodeget-server_uuid RPC 方法获取，或在 Server 配置文件中查看
server_uuid = "00000000-0000-0000-0000-000000000000"

# 具有一定权限的 Token，可以为 TokenKey:TokenSecret 或 Username|Password
token = "test_server1_token"

# Server 的 WebSocket 地址，必须携带协议头
ws_url = "ws://127.0.0.1:2211/"

# 是否允许执行任务
allow_task = true

# 是否允许 ICMP Ping
allow_icmp_ping = true

# 是否允许 TCP Ping
allow_tcp_ping = true

# 是否允许 HTTP Ping
allow_http_ping = true

# 是否允许通用 HTTP 请求，危险操作，谨慎开启
allow_http_request = true

# 是否允许 Web Shell，极度危险，谨慎开启
allow_web_shell = true

# 是否允许执行命令，极度危险，谨慎开启
allow_execute = true

# 是否允许阅读配置，极度危险，谨慎开启
allow_read_config = true

# 是否允许编辑配置，极度危险，谨慎开启
allow_edit_config = true

# 是否允许获取 IP 地址
allow_ip = true

# 是否允许 DNS 查询
# 支持 A / AAAA / CNAME / MX / TXT / NS / SRV / PTR / SOA / CAA 记录类型
# Agent 使用 hickory-resolver 库执行 DNS 查询，支持自定义 DNS 服务器（格式 "IP:port"）或系统默认
# 建议按需开启
allow_dns = true

# 是否允许获取 Agent 版本信息
allow_version = true

# 是否允许自更新，开启后 Server 可通过 SelfUpdate 任务触发 Agent 自动下载并替换二进制
# 支持升级和降级，版本号仅做格式校验（vX.Y.Z）
# 重启后生效，需谨慎开启
# 注意：Unix 平台使用 execv 替换当前进程（不创建新进程）
# 注意：Windows 平台拉起新进程后自身退出
allow_self_update = false

# 允许执行的任务类型白名单（可选）
# 若指定此列表且非空，则单独的任务开关（如 allow_ping / allow_execute 等）全部失效
# 以本列表为准，未列出的任务类型一律拒绝
# 值为 task_name，可选值包括：ping / tcp_ping / http_ping / dns / execute / http_request
# / web_shell / read_config / edit_config / ip / version / self_update
# allow_task_type = ["ping", "dns", "ip"]

# 是否忽略服务端 TLS 证书校验，默认关闭
# 仅在 Server 使用自签名证书或测试环境时开启
# 开启后将不再校验服务端证书链和主机名，存在中间人攻击风险，生产环境请谨慎使用
ignore_cert = false


# 第二个 Server
[[server]]
name = "test_server2"
server_uuid = "00000000-0000-0000-0000-000000000000"
token = "test_server2_token"
ws_url = "ws://nodeget-secondary.example.com:2211/"
```

## `dynamic_summary_select_disk` 与 `dynamic_summary_select_network_interface`

这两个字段用于控制 **Dynamic Summary**（动态监控摘要）中磁盘和网卡的统计范围，属于**可选配置**，留空或注释掉时会回退到默认行为。

- **`dynamic_summary_select_disk`**：按磁盘的 `mount_point`（挂载点）进行白名单匹配，例如 `["/", "/data"]` 表示只统计根目录和
  `/data` 的磁盘数据
- **`dynamic_summary_select_network_interface`**：按网卡的 `interface_name`（接口名）进行白名单匹配，例如 `["eth0", "eth1"]`
  表示只统计 `eth0` 和 `eth1` 的网络流量

### 回退行为

- 若字段存在且数组**非空**，则仅统计列表中指定的项，其他项不参与汇总计算
- 若字段不存在、为空数组，或被注释掉，则回退到默认排除逻辑：
    - **磁盘**：按挂载点前缀自动过滤 `/tmp`、`/var/tmp`、`/dev`、`/run`、`/var/lib/containers`、`/var/lib/docker`、`/proc`、`/sys`、`/sys/fs/cgroup`、`/etc/resolv.conf`、`/etc/host`、`/nix/store` 等虚拟/临时挂载点
    - **网卡**：按接口名前缀自动过滤 `br`、`cni`、`docker`、`podman`、`flannel`、`lo`、`veth`、`virbr`、`vmbr`、`tap`、`fwbr`、`fwpr` 等虚拟/隧道网卡

### 使用场景

适合需要精确控制 Summary 数据的场景，例如：

- 服务器只有部分磁盘需要监控（如只关注数据盘而排除系统盘）
- 云服务器有多张网卡，只希望统计特定网卡流量（如只统计公网网卡）
- 默认排除规则过滤了你实际想监控的项（如自定义命名的虚拟网卡）
