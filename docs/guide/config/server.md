# Server 配置

```toml
# 日志等级，可选 trace / debug / info / warn / error，默认 info
# 如果你正在测试或遇到问题，请至少选择 debug
log_level = "debug"

# WebSocket 监听地址，同时也会监听 Http 服务
ws_listener = "0.0.0.0:3000"

# JSON-RPC 最大连接数，默认 100
jsonrpc_max_connections = 100

# JSON-RPC 单次请求耗时日志级别（call / batch / notification）
# 可选 trace / debug / info / warn / error，默认 trace
# 该项独立于 log_level，仅控制 RPC 耗时日志
jsonrpc_timing_log_level = "trace"

# 是否启用 Unix Socket（仅非 Windows 平台）
# 启用后会额外监听 unix_socket_path 对应的 Axum 主路由
enable_unix_socket = false

# Unix Socket 路径（仅非 Windows 平台）
# 若 enable_unix_socket = true 且不配置该项，则默认 /var/lib/nodeget.sock
unix_socket_path = "/var/lib/nodeget.sock"

# Server 的 Uuid，建议设置为 auto_gen 以自动生成，根据系统环境自动生成，可保证数据不冲突（概率极小）
# 如果不是 auto_gen，请自行确保 Uuid 唯一，否则可能导致数据混乱或 UB
server_uuid = "auto_gen"

# 数据库配置
[database]

# 数据库地址
# 目前支持 sqlite / pgsql
# 优先使用 pgsql
# sqlite 示例: sqlite://nodeget.db?mode=rwc
# pgsql 示例: postgres://user:pass@host:5432/nodeget
database_url = "postgres://user:pass@host:5432/nodeget"

# SQLx 日志级别
# 可选 trace / debug / info / warn / error，默认 info
# 若此项低于 log_level，则不会打印 SQL 执行日志
sqlx_log_level = "info"

# 连接超时，单位毫秒，默认 3000
connect_timeout_ms = 3000

# 获取连接超时，单位毫秒，默认 3000
acquire_timeout_ms = 3000

# 空闲超时，单位毫秒，默认 3000
idle_timeout_ms = 3000

# 最大生命周期，单位毫秒，默认 30000
max_lifetime_ms = 30000

# 最大连接数，默认 10
# 若是大型服务器，请务必调大该项，否则将严重影响性能
# 若是小型服务器，保持默认即可，过大该数值可能导致内存占用激增
max_connections = 10
```

## Unix Socket 说明

- 仅在非 Windows 平台生效。
- `enable_unix_socket = true` 时，Server 会在保留原 `ws_listener` 的同时，额外监听 `unix_socket_path`。
- `unix_socket_path` 未配置时默认 `/var/lib/nodeget.sock`。
- Unix Socket 与 TCP 共享同一套 Axum 主路由（包括 JSON-RPC HTTP 路由与 `/worker-route/*`）。
- 启动时会尝试移除已有同名 socket 文件；服务重载/退出时会清理 socket 文件。

示例（通过 Unix Socket 调用 JSON-RPC）：

```bash
curl --unix-socket /var/lib/nodeget.sock \
  -H "content-type: application/json" \
  -X POST http://localhost/ \
  -d '{"jsonrpc":"2.0","method":"nodeget-server_hello","params":[],"id":1}'
```

若希望仅使用 Unix Socket，可将 `ws_listener` 绑定到本地回环地址并配合防火墙策略限制外部访问。
