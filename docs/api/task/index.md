# Task 任务总览

Task 是本项目的重要功能之一，也可以称为 `任务` 等

## 方法列表

| 方法名                                                         | 描述                   |
|-------------------------------------------------------------|----------------------|
| [task_create_task](./crud.md#create-task)                   | 创建并下发任务给 Agent       |
| [task_create_task_blocking](./crud.md#create-task-blocking) | 创建任务并阻塞等待 Agent 返回结果 |
| [task_query_task](./crud.md#query-task)                     | 查询任务执行记录             |
| [task_delete_task](./crud.md#delete-task)                   | 删除任务执行记录             |

Agent 端实现请参考 [agent.md](./agent.md)。

## 基本流程

Agent 可以接收来自 Server 的 Task（任务），并可以为 Server 配置简单的权限

基本路线:

```
Agent 事先向 Server 发送 Task 订阅请求，在长连接中处理

调用者发送任务 / 定时任务 => Server => 储存到数据库 => Agent 获取执行 => 返回给 Server => 数据库保存

调用者可用 JSON-RPC API 获取或删除任务执行记录
```

## 任务主体

最重要的结构体如下，其为 Rust Enum，解析到 Json 用于传输:

```rust
#[serde(rename_all = "snake_case")]
pub enum TaskEventType {
    Ping(String),       // 可能为域名，需解析
    TcpPing(String),    // 可能为域名，需解析
    HttpPing(url::Url), // Url, Method, Body
    HttpRequest(HttpRequestTask), // 通用 HTTP 请求

    WebShell(WebShellTask), // Websocket URL + terminal_id
    Execute(ExecuteTask), // 结构化命令执行
    ReadConfig,         // 读取本地 config.toml
    EditConfig(String), // 编辑本地 config.toml（完整 TOML 字符串）

    Ip,

    Version,            // 获取 Agent 版本信息
}

pub struct WebShellTask {
    pub url: url::Url,
    pub terminal_id: uuid::Uuid,
}

pub struct ExecuteTask {
    pub cmd: String,
    pub args: Vec<String>,
}

pub struct HttpRequestTask {
    pub url: url::Url,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>, // 与 body_base64 互斥
    pub body_base64: Option<String>, // 与 body 互斥
    pub ip: Option<String>, // 指定出口 IP，或 "ipv4 auto" / "ipv6 auto"
}
```

下面是一些解析的示例:

```json
{
  "ping": "1.1.1.1" // 目标地址，可能为域名
}

{
  "tcp_ping": "1.1.1.1:80" // 目标地址:端口
}

{
  "http_ping": "https://1.1.1.1/" // 完整 URL
}

{
  "http_request": { // 通用 HTTP 请求
    "url": "https://example.com", // 完整 URL
    "method": "POST", // HTTP 方法
    "headers": { // 请求头
      "content-type": "application/json"
    },
    "body": "{\"hello\":\"world\"}", // 与 body_base64 互斥
    "ip": "ipv4 auto" // 指定出口 IP，可选
  }
}

{
  "web_shell": { // WebShell 任务
    "url": "wss://example.com/auto_gen", // WebSocket URL
    "terminal_id": "4c8d1cba-244e-4baf-9b65-c881f86ca60a" // 随机 UUID
  }
}

{
  "execute": { // 结构化命令执行
    "cmd": "ls", // 命令名，不能为空字符串
    "args": [ // 参数列表
      "-1",
      "tmp"
    ]
  }
}

"read_config" // 读取本地 config.toml

{
  "edit_config": "log_level = \"info\"\\nagent_uuid = \"auto_gen\"" // 完整 TOML 字符串
}

"ip" // 对就是一个 `ip`，无其他东西

"version" // 获取 Agent 版本信息
```

### 注意事项

`execute` 不再提供字符串拼接 shell 的直接接口。如果你确实需要 shell 语法，请显式调用 shell 程序并传参数，例如：

```json
{
  "execute": {
    "cmd": "bash",
    "args": [
      "-c",
      "ls -l /tmp"
    ]
  }
}
```

`execute.cmd` 不能为空字符串

`http_request` 中 `body` 与 `body_base64` 互斥，最多只能出现一个字段

## 任务回报

Agent 在执行完后需要通过该结构体返回数据

```rust
#[serde(rename_all = "snake_case")]
pub enum TaskEventResult {
    Ping(f64),     // 延迟
    TcpPing(f64),  // 延迟
    HttpPing(f64), // 延迟
    HttpRequest(HttpRequestTaskResult), // HTTP 请求结果

    WebShell(bool),  // Is Connected
    Execute(String), // 命令输出
    ReadConfig(String), // 当前 config.toml 原文
    EditConfig(bool),   // 是否成功写入

    Ip(Option<Ipv4Addr>, Option<Ipv6Addr>), // V4 V6 IP

    Version(NodeGetVersion), // Agent 版本信息
}

pub struct HttpRequestTaskResult {
    pub status: u16,
    pub headers: Vec<BTreeMap<String, String>>, // 数组格式，允许重复 key
    pub body: Option<String>, // 与 body_base64 互斥
    pub body_base64: Option<String>, // 与 body 互斥
}
```

在同一个 Task 中，enum 名需要匹配

下面是一些解析的示例:

```json
{
  "ping": 114.51 // 延迟，单位 ms
}

{
  "execute": "WE LOVE OPEN-SOURCE" // 命令输出，包含于一个 String 内
}

{
  "http_request": { // HTTP 请求结果
    "status": 200, // HTTP 状态码
    "headers": [ // 数组格式，允许重复 key
      {
        "content-type": "application/json"
      }
    ],
    "body": "{\"hello\":\"world\"}" // 与 body_base64 互斥
  }
}

{
  "read_config": "log_level = \"info\"\\nagent_uuid = \"auto_gen\"" // config.toml 原文
}

{
  "edit_config": true // 是否成功写入
}

{
  "ip": [ // V4 V6 IP
    "1.1.1.1",
    "2606:4700:4700::1111"
  ]
}
```

### 注意事项

若响应体不是 UTF-8 文本，则会返回 `body_base64`，并且不会返回 `body`

在同一个 Task 中，`TaskEventResult` 的 enum 变体需要与 `TaskEventType` 匹配

## 查询条件

需要用到统一的结构体 `TaskQueryCondition`

其为 Rust Enum，解析时请注意:

```rust
#[serde(rename_all = "snake_case")]
pub enum TaskQueryCondition {
    TaskId(u64),
    Uuid(uuid::Uuid),
    TimestampFromTo(i64, i64), // start, end
    TimestampFrom(i64),        // start,
    TimestampTo(i64),          // end

    IsSuccess,    // 仅查找 success 字段为 true
    IsFailure,    // 仅查找 success 字段为 false
    IsRunning,    // 仅查找 success 字段为空
    Type(String), // task_event_type 中有字段为 `String` 的行
    CronSource(String), // 仅查找由指定 cron name 创建的任务

    Limit(u64), // limit

    Last,
}
```

下面是一些解析的示例:

```json
{
    "task_id": 42 // 按数据库 ID 查询
}

{
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" // 按 Agent UUID 查询
}

{
    "timestamp_from_to": [1769344168646, 1769344169646] // 时间范围
}

{
    "timestamp_from": 1769344168646 // 起始时间
}

{
    "limit": 1000 // 依照 timestamp 最新的 1000 条
}

"is_success" // 仅查找成功的任务

"is_failure" // 仅查找失败的任务

"is_running" // 仅查找运行中的任务

{
    "type": "ping" // 按任务类型查询
}

{
    "cron_source": "daily_check" // 按 cron 名称查询
}

"last" // 对就是一个 `last`，无其他东西
```

### 注意事项

`timestamp_from_to` 字段可看作是 `timestamp_from` 与 `timestamp_to` 的简略写法，下面的两种表达方式是等价的:

```json
{
    "timestamp_from_to": [1769344168646, 1769344169646]
}

[
    {
        "timestamp_from": 1769344168646
    },
    {
        "timestamp_to": 1769344169646
    }
]
```

`limit` 为 1 与 `last` 等价，在数据库层面限制查询结果，按照时间倒序排列

多个条件并存时，为 `AND`，即只查询满足所有条件的数据
