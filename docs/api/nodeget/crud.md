# NodeGet CRUD

## Hello

测试服务是否正常运行，返回固定字符串。

### 方法

调用方法名为 `nodeget-server_hello`，无需任何参数。

### 权限要求

该方法不需要鉴权，可直接调用。

### 返回值

返回固定字符串 `"NodeGet Server Is Running!"`，可用于判断服务端是否在线。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_hello",
  "params": [],
  // 无参数
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": "NodeGet Server Is Running!",
  "id": 1
}
```

## Version

获取服务端的版本、构建、编译器等详细信息。

### 方法

调用方法名为 `nodeget-server_version`，无需任何参数。

### 权限要求

该方法不需要鉴权，可直接调用。

### 返回值

返回 `NodeGetVersion` 结构体，包含完整的编译时信息，所有字段均为字符串类型，详细说明参考 [NodeGet 总览](./index.md)。

```json
{
  "binary_type": "Server",                    // 二进制类型
  "build_time": "2026-02-08T10:44:02.848471700Z", // 构建时间
  "cargo_target_triple": "x86_64-pc-windows-msvc", // 编译目标
  "cargo_version": "0.0.1",                   // Cargo 版本号
  "git_branch": "main",                       // Git 分支
  "git_commit_date": "2026-02-08T07:25:09.000000000Z", // 提交日期
  "git_commit_message": "Feat: ...",           // 提交信息
  "git_commit_sha": "73d9589",                // 提交 SHA
  "rustc_channel": "nightly",                 // Rust 编译器通道
  "rustc_commit_date": "2025-12-30",          // Rust 编译器提交日期
  "rustc_commit_hash": "0e8999942552691afc20495af6227eca8ab0af05", // Rust 编译器提交 Hash
  "rustc_llvm_version": "21.1",               // LLVM 版本
  "rustc_version": "1.94.0-nightly"           // Rust 版本
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_version",
  "params": [], // 无参数
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "binary_type": "Server",
    "build_time": "2026-02-08T10:44:02.848471700Z",
    "cargo_target_triple": "x86_64-pc-windows-msvc",
    "cargo_version": "0.0.1",
    "git_branch": "main",
    "git_commit_date": "2026-02-08T07:25:09.000000000Z",
    "git_commit_message": "Feat: ...",
    "git_commit_sha": "73d9589",
    "rustc_channel": "nightly",
    "rustc_commit_date": "2025-12-30",
    "rustc_commit_hash": "0e8999942552691afc20495af6227eca8ab0af05",
    "rustc_llvm_version": "21.1",
    "rustc_version": "1.94.0-nightly"
  },
  "id": 1
}
```

## UUID

获取当前 Server 的 UUID。

### 方法

调用方法名为 `nodeget-server_uuid`，无需任何参数。

### 权限要求

该方法不需要鉴权，可直接调用。

### 返回值

返回当前服务端的 UUID 字符串，该 UUID 在配置文件中通过 `server_uuid` 字段设定。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_uuid",
  "params": [], // 无参数
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": "e8583352-39e8-5a5b-b66c-e450689088fd", // Server UUID
  "id": 1
}
```

## List All Agent UUID

获取 Server 中所有 Agent 的 UUID 列表。

### 方法

调用方法名为 `nodeget-server_list_all_agent_uuid`，需要提供以下参数：

```json
{
  "token": "demo_token" // Token 字符串
}
```

### 权限要求

- Permission: `NodeGet::ListAllAgentUuid`
- Scope 行为:
    - `Global` Scope 下拥有该权限: 返回系统内所有 Agent UUID
    - `AgentUuid(xxx)` Scope 下拥有该权限: 可参与返回 `xxx`
    - 最终返回结果会再过滤为"当前 token 在该 `AgentUuid` 下至少有一种可操作权限（任一非 `NodeGet::ListAllAgentUuid`
      权限）"的 UUID

### 返回值

返回包含 `uuids` 字段的对象，其值为 `Vec<Uuid>` 数组。

该方法会从以下四个来源获取所有不同的 Agent UUID:

1. `static_monitoring` - 静态监控数据表（查询 `uuid_id` 后映射为 UUID）
2. `dynamic_monitoring` - 动态监控数据表（查询 `uuid_id` 后映射为 UUID）
3. `dynamic_monitoring_summary` - 动态监控摘要数据表（查询 `uuid_id` 后映射为 UUID）
4. `task` - 任务数据表（直接查询 UUID）

返回的 UUID 列表是去重后按字母顺序排序的。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_list_all_agent_uuid",
  "params": {
    "token": "demo_token"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "uuids": [
      "e8583352-39e8-5a5b-b66c-e450689088fd",
      "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d"
    ]
  },
  "id": 1
}
```

## Read Config

读取当前 Server 使用的配置文件原文（`config.toml` 文本）。

### 方法

调用方法名为 `nodeget-server_read_config`，需要提供以下参数：

```json
{
  "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET" // SuperToken 字符串
}
```

### 权限要求

该方法仅允许 **SuperToken** 调用。

`token` 支持以下格式之一:

- `token_key:token_secret`
- `username|password`

### 返回值

返回配置文件在磁盘上的原始文本内容，为 String 类型。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_read_config",
  "params": {
    "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": "ws_listener = \"0.0.0.0:2211\"\\nserver_uuid = \"auto_gen\"\\n\\n[logging]\\nlog_filter = \"info\"\\n\\n[database]\\ndatabase_url = \"sqlite://nodeget.db?mode=rwc\"\\n...", // 配置文件原始文本
  "id": 1
}
```

## Edit Config

写入新的 Server 配置文本，并触发服务端配置热重载。

### 方法

调用方法名为 `nodeget-server_edit_config`，需要提供以下参数：

```json
{
  "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET", // SuperToken 字符串
  "config_string": "ws_listener = \"0.0.0.0:2211\"\\n..."   // 完整的 TOML 配置文本
}
```

### 权限要求

该方法仅允许 **SuperToken** 调用。

`token` 支持以下格式之一:

- `token_key:token_secret`
- `username|password`

### 返回值

返回 `bool` 类型，`true` 表示配置写入成功并已触发热重载。

行为说明：

- 服务端会先校验 `config_string` 是否是可解析的 Server TOML 配置
- 校验通过后写入配置文件
- 写入成功后触发配置重载流程

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_edit_config",
  "params": {
    "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET",
    "config_string": "ws_listener = \"0.0.0.0:2211\"\\nserver_uuid = \"auto_gen\"\\njsonrpc_max_connections = 100\\n\\n[logging]\\nlog_filter = \"info\"\\n\\n[database]\\ndatabase_url = \"sqlite://data/server.db?mode=rwc\""
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": true, // 写入成功
  "id": 1
}
```

## Database Storage

查询数据库中各业务表的存储占用大小（字节）。

### 方法

调用方法名为 `nodeget-server_database_storage`，需要提供以下参数：

```json
{
  "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET" // SuperToken 字符串
}
```

### 权限要求

该方法仅允许 **SuperToken** 调用。

`token` 支持以下格式之一:

- `token_key:token_secret`
- `username|password`

### 返回值

返回包含 `tables` 和 `total` 两个字段的对象，详细说明参考 [NodeGet 总览](./index.md)。

- `tables`: 各表名到存储大小（字节）的映射，按表名字母顺序排列
- `total`: 所有表存储大小之和（字节）

查询范围为 9 张业务表，不含 `seaql_migrations`。

不同数据库后端的查询方式不同:

- **PostgreSQL**: 使用 `pg_total_relation_size()` 获取各表总大小（含索引和 TOAST 数据）
- **SQLite**: 使用 `dbstat` 虚拟表查询各表占用的页面总大小

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_database_storage",
  "params": {
    "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "tables": {
      "crontab": 4096,
      "crontab_result": 8192,
      "dynamic_monitoring": 16384,
      "js_result": 4096,
      "js_worker": 4096,
      "kv": 8192,
      "static_monitoring": 8192,
      "task": 4096,
      "token": 4096
    },
    "total": 61440
  },
  "id": 1
}
```

## Log

查询服务端内存日志缓冲区中的所有日志条目。

### 方法

调用方法名为 `nodeget-server_log`，需要提供以下参数：

```json
{
  "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET"
  // SuperToken 字符串
}
```

### 权限要求

该方法仅允许 **SuperToken** 调用。

`token` 支持以下格式之一:

- `token_key:token_secret`
- `username|password`

### 返回值

返回一个 JSON 数组，每个元素为一条日志记录，包含以下字段:

- `timestamp`: ISO 8601 格式的时间戳（含时区）
- `level`: 日志级别（`TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`）
- `target`: 日志 target（数据库相关日志统一重映射为 `"db"`）
- `message`: 日志消息文本
- `fields`: 结构化字段对象（无额外字段时为空对象 `{}`）
- `spans`: span 上下文数组（无 span 时为空数组 `[]`）

缓冲区为固定容量的环形缓冲区，满时自动淘汰最旧的条目。返回顺序为时间正序（最旧在前）。

容量和过滤级别可在 `config.toml` 的 `[logging]` 段中通过 `memory_log_capacity` 和 `memory_log_filter` 配置。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_log",
  "params": {
    "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "timestamp": "2026-04-11T12:00:00.000+08:00",
      "level": "INFO",
      "target": "server",
      "message": "Starting nodeget-server",
      "fields": {},
      "spans": []
    },
    {
      "timestamp": "2026-04-11T12:00:01.234+08:00",
      "level": "DEBUG",
      "target": "rpc",
      "message": "success",
      "fields": {},
      "spans": [
        {
          "name": "kv::get_value",
          "fields": "token_key=demo namespace=test key=foo"
        }
      ]
    },
    {
      "timestamp": "2026-04-11T12:00:01.240+08:00",
      "level": "DEBUG",
      "target": "db",
      "message": "SELECT \"kv\".\"key\", \"kv\".\"value\" FROM \"kv\" WHERE \"kv\".\"namespace\" = $1 AND \"kv\".\"key\" = $2",
      "fields": {},
      "spans": []
    }
  ],
  "id": 1
}
```

## Stream Log

实时订阅服务端日志流。该方法是 JSON-RPC **Subscription**（基于 WebSocket），建立订阅后服务端会持续推送匹配过滤条件的日志事件。

### 方法

订阅方法名为 `nodeget-server_stream_log`，取消订阅方法名为 `nodeget-server_unsubscribe_stream_log`。

需要提供以下参数：

```json
{
  "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET", // SuperToken 字符串
  "log_filter": "info,rpc=debug,db=trace"        // 日志过滤规则，语法同 RUST_LOG
}
```

`log_filter` 参数说明:

- 语法与 `RUST_LOG` 环境变量相同，支持 `target=level` 的逗号分隔组合
- 支持虚拟 target `db`，会自动展开为 `sea_orm` / `sea_orm_migration` / `sqlx`
- 可用的 target: `server`, `rpc`, `db`, `kv`, `monitoring`, `task`, `token`, `js_worker`, `js_result`, `crontab`,
  `crontab_result`, `js_runtime`, `terminal`
- 示例: `"info"` 接收所有 INFO 及以上级别，`"debug,db=trace"` 接收 DEBUG 级别 + 数据库 TRACE 级别

### 权限要求

该方法仅允许 **SuperToken** 调用。

`token` 支持以下格式之一:

- `token_key:token_secret`
- `username|password`

认证失败时，服务端会拒绝订阅请求（reject），WebSocket 连接不会建立订阅通道。

### 返回值

订阅建立成功后，服务端通过 WebSocket 持续推送 JSON-RPC notification，每条 notification 的 `params.result`
为一个日志事件对象:

- `timestamp`: ISO 8601 格式的时间戳（含时区）
- `level`: 日志级别（`TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`）
- `target`: 日志 target（数据库相关日志统一重映射为 `"db"`）
- `message`: 日志消息文本
- `fields`: 结构化字段对象（无额外字段时为空对象 `{}`）
- `spans`: span 上下文数组（无 span 时为空数组 `[]`）

### 行为说明

- 订阅建立后，仅推送**新产生**的日志事件（不回放历史日志，历史日志请使用 `log` 方法查询）
- 每个订阅者拥有独立的 512 容量 channel 缓冲区，当客户端消费速度过慢导致缓冲区满时，新日志会被丢弃
- 客户端断开 WebSocket 连接或发送取消订阅请求后，服务端自动清理对应订阅
- 支持多个客户端同时订阅，各订阅者的 `log_filter` 互相独立

### 完整示例

订阅请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_stream_log",
  "params": {
    "token": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET",
    "log_filter": "info,rpc=debug"
  },
  "id": 1
}
```

订阅成功响应（返回 subscription ID）:

```json
{
  "jsonrpc": "2.0",
  "result": "subscription_id_here",
  "id": 1
}
```

后续推送的日志事件（JSON-RPC notification）:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_stream_log",
  "params": {
    "subscription": "subscription_id_here",
    "result": {
      "timestamp": "2026-04-11T12:00:05.678+08:00",
      "level": "INFO",
      "target": "server",
      "message": "config reloaded successfully",
      "fields": {},
      "spans": []
    }
  }
}
```

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_stream_log",
  "params": {
    "subscription": "subscription_id_here",
    "result": {
      "timestamp": "2026-04-11T12:00:06.123+08:00",
      "level": "DEBUG",
      "target": "rpc",
      "message": "success",
      "fields": {},
      "spans": [
        {
          "name": "kv::set_value",
          "fields": "token_key=demo namespace=config key=theme"
        }
      ]
    }
  }
}
```

取消订阅请求:

```json
{
  "jsonrpc": "2.0",
  "method": "nodeget-server_unsubscribe_stream_log",
  "params": {
    "subscription": "subscription_id_here"
  },
  "id": 2
}
```

取消订阅响应:

```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 2
}
```
