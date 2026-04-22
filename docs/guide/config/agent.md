# Agent 配置

官方脚本安装的 nodeget-agent 的配置路径位于  /etc/nodeget-agent.conf

```toml
# 日志等级，可选 trace / debug / info / warn / error，默认 info
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

# Agent 的 Uuid，建议设置为 auto_gen 以自动生成，根据系统环境自动生成，可保证数据不冲突（概率极小）
# 如果不是 auto_gen，请自行确保每个 Agent 的 uuid 唯一，否则可能导致数据混乱或 UB
agent_uuid = "auto_gen"

# 连接超时时间（毫秒），默认 1000
connect_timeout_ms = 1000

# 终端 Shell，Linux 下默认 Bash，Windows 下默认 CMD
terminal_shell = "bash"

# 执行命令输出的最大字符数量限制
# 超出该数量只返回命令的最后结果，上文将被截断，默认 10000
exec_max_character = 10000

# IP 地址获取服务提供商，可选 ipinfo / cloudflare，默认 ipinfo
ip_provider = "ipinfo"

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

# Server 的 Websocket 地址，必须携带协议头
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


# 第二个 Server
[[server]]
name = "test_server2"
server_uuid = "00000000-0000-0000-0000-000000000000"
token = "test_server2_token"
ws_url = "ws://127.0.0.1:3000/"
```
