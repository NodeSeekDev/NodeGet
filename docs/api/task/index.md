# Task 任务总览

Task 是本项目的重要功能之一，也可以称为 `任务` 等

## 基本流程

Agent 可以接收来自 Server 的 Task (任务)，并可以为 Server 配置简单的权限

基本路线:

```
Agent 事先向 Server 发送 Task 订阅请求，在长连接中处理

调用者发送任务 / 定时任务 => Server => 储存到数据库 => Agent 获取执行 => 返回给 Server => 数据库保存

调用者可用 JsonRpc API 获取任务执行情况
```

## 任务主体

最重要的结构体如下，其为 Rust Enum，解析到 Json 用于传输:

```rust
#[serde(rename_all = "snake_case")]
pub enum TaskEventType {
    Ping(String),       // 可能为域名，需解析
    TcpPing(String),    // 可能为域名，需解析
    HttpPing(url::Url), // Url, Method, Body

    WebShell(WebShellTask), // Websocket URL + terminal_id
    Execute(String),    // 命令执行
    ReadConfig,         // 读取本地 config.toml
    EditConfig(String), // 编辑本地 config.toml（完整 TOML 字符串）

    Ip,
}

pub struct WebShellTask {
    pub url: url::Url,
    pub terminal_id: uuid::Uuid,
}
```

下面是一些解析的示例:

```json
{
  "ping": "1.1.1.1"
}

{
  "tcp_ping": "1.1.1.1:80"
}

{
  "http_ping": "https://1.1.1.1/"
}

{
  "web_shell": {
    "url": "wss://example.com/auto_gen",
    "terminal_id": "4c8d1cba-244e-4baf-9b65-c881f86ca60a"
  }
}

{
  "execute": "echo 'WE LOVE OPEN-SOURCE'"
}

"read_config"

{
  "edit_config": "log_level = \"info\"\\nagent_uuid = \"auto_gen\""
}

"ip" // 对就是一个 `ip`，无其他东西
```

## 任务回报

Agent 在执行完后需要通过该结构体返回数据

```rust
#[serde(rename_all = "snake_case")]
pub enum TaskEventResult {
    Ping(f64),     // 延迟
    TcpPing(f64),  // 延迟
    HttpPing(f64), // 延迟

    WebShell(bool),  // Is Connected
    Execute(String), // 命令输出
    ReadConfig(String), // 当前 config.toml 原文
    EditConfig(bool),   // 是否成功写入

    Ip(Option<Ipv4Addr>, Option<Ipv6Addr>), // V4 V6 IP
}
```

在同一个 Task 中，enum 名需要匹配

下面是一些解析的示例:

```json
{
  "ping": 114.51
}

{
  "execute": "WE LOVE OPEN-SOURCE"
  // 在执行复杂命令时还有其他的部分，不过都包含于一个 String 内
}

{
  "read_config": "log_level = \"info\"\\nagent_uuid = \"auto_gen\""
}

{
  "edit_config": true
}

{
  "ip": [
    "1.1.1.1",
    "2606:4700:4700::1111"
  ]
}
```

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

    Limit(u64), // limit

    Last,
}
```

解析方案与 Monitoring 的 `QueryCondition` 类似，不做示例

多个条件并存时，为 `AND`，即只查询满足所有条件的数据
