# Token 总览

Token 是本项目的鉴权核心，任何有权限的操作都应持有 有对应权限的 Token

## Token 分类

在本项目，Token 可以分为两类

- SuperToken: 在 Server 初始化时创建的唯一值，数据库 ID 为 1 的 Token，在所有操作中该 Token 直接放行
- Token: 由 SuperToken 创建的子 Token

Token 可以是下列值:

- `TOKEN_KEY:TOKEN_SECRET`: Token Key 明文储存，Token Secret 为主要鉴权部分
- `Username|Password`: Username 明文储存，Password 为主要鉴权部分

区别位于分隔符不同，在 Username+Password 方案中，只取第一个分隔符 `|`，后面作为 Password

特点:

- Token 与 Username+Password 等价，但 Server 内部鉴权只有 Token。在任何 API 中两种形式均可
- Token 与 Username 一一对应，SuperToken 对应的 Username 为 root
- Token 不可变且不可指定，但 Username+Password 可以自行更改

## 基本结构

一个 Token 对应如下结构体:

```rust
pub struct Token {
    pub version: u8, // 暂时为 1
    pub token_key: String, // 标识 Token 最主要的键
    pub timestamp_from: Option<i64>, // Token 有效期
    pub timestamp_to: Option<i64>,
    pub token_limit: Vec<Limit>, // 权限范围
    pub username: Option<String>, // 用户名
}
```

Token Secret 与 Password 存于数据库中，无反向解析

一个 Token 可以对应多个 Limit，在不同的作用域 (Scope) 下有不同的权限 (Permission)

### Limit

一个 Limit 对应多个 Scope 与 Permission

```rust
pub struct Limit {
    pub scopes: Vec<Scope>,
    pub permissions: Vec<Permission>,
}
```

### Scope

Scope 为作用域，即表示在某一个对象 (目前为 Agent Uuid) 有权限

```rust
pub enum Scope {
    Global, // 全局 Agent Uuid
    AgentUuid(uuid::Uuid),
}
```

### Permission

```rust
pub enum Permission {
    StaticMonitoring(StaticMonitoring),
    DynamicMonitoring(DynamicMonitoring),
    Task(Task),
}

pub enum StaticMonitoring {
    Read(StaticDataQueryField),
    Write,
}

pub enum DynamicMonitoring {
    Read(DynamicDataQueryField),
    Write,
}

// Type 字段名
// 接受 ping / tcp_ping / http_ping / web_shell / execute / ip
pub enum Task {
    Create(String), // 创建权限
    Read(String),
    Write(String), // 上报权限
    Listen, // 监听权限
}
```

若存在于 Limit 的 permissions 中，即为拥有该权限

## Demo

### Agent 基础

现有这么一个结构体

```json
{
  "scopes": [
    {"agent_uuid": "adf78235-a23c-46fc-bc85-694f64c39aaf"},
    {"agent_uuid": "33c1b63a-35f1-4b9f-9659-66e7a3e5a75c"}
  ],
  "permissions": [
    {"dynamic_monitoring": "write"},
    {"static_monitoring": "write"},

    {"task": "listen"},

    {"task": {"write": "ping"}},
    {"task": {"write": "tcp_ping"}},
    {"task": {"write": "http_ping"}},
    {"task": {"write": "web_shell"}},
    {"task": {"write": "execute"}},
    {"task": {"write": "ip"}}
  ]
}
```

这是一个 Agent 能正常调用所有功能的 Limit，它表示:

Agent Uuid 为 `ad..af` 与 `33..5c` 的 Agent，具有上传 StaticMonitoring / DynamicMonitoring 数据、监听 Server 下发 Task、上报目前所有 Task 任务类型 的权限

### 查询 基础

现有这么一个结构体

```json
{
  "scopes": [
    {"agent_uuid": "53f125b6-e7aa-447f-a27c-085a53a36462"},
    {"agent_uuid": "3e6f227f-56e3-4ca0-a12f-04014ebeebe7"}
  ],
  "permissions": [
    {"dynamic_monitoring": {"read": "cpu"}},
    {"dynamic_monitoring": {"read": "system"}},
    {"static_monitoring": {"read": "cpu"}},
    {"static_monitoring": {"read": "system"}}
  ]
}
```

它表示:

用户可以查询 Agent Uuid 为 `ad..af` 与 `33..5c` 的 Agent 的 StaticMonitoring / DynamicMonitoring Data 中 cpu / system 字段