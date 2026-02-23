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
    pub timestamp_from: Option<i64>, // Token 有效期，毫秒时间戳
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
    // 全局作用域，适用于所有地点
    Global,
    // 特定 Agent 作用域，通过 UUID 指定
    AgentUuid(uuid::Uuid),
    // KvNamespace 作用域，通过名称指定
    KvNamespace(String),
}
```

### Permission

```rust
// 权限枚举，定义不同类型的操作权限
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    // 静态监控权限
    StaticMonitoring(StaticMonitoring),
    // 动态监控权限
    DynamicMonitoring(DynamicMonitoring),
    // 任务权限
    Task(Task),
    // Crontab 权限
    Crontab(Crontab),

    // Kv 权限
    Kv(Kv),

    // Terminal 权限
    Terminal(Terminal),

    // CrontabResult 权限
    CrontabResult(CrontabResult),
    
    // NodeGet 权限
    NodeGet(NodeGet),
}

// 静态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    // 读取权限，指定可读取的字段类型
    Read(StaticDataQueryField),
    // 写入权限
    Write,
}

// 动态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    // 读取权限，指定可读取的字段类型
    Read(DynamicDataQueryField),
    // 写入权限
    Write,
}

// 任务权限枚举
// Type 字段名
// 接受 ping / tcp_ping / http_ping / web_shell / execute / ip
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    // 创建权限，指定任务类型
    Create(String),
    // 读取权限，指定任务类型
    Read(String),
    // 写入权限，指定任务类型
    Write(String),
    // 监听权限
    Listen,
}

// Crontab 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crontab {
    // 可以读取在自己 Scope 下的所有 Crontab
    Read,
    // 可以创建 Crontab
    // 若 Crontab 类型为下发给 Agent 任务，则该 Token 还必须拥有对应 Agent 的 Task Create 权限
    // 若 Crontab 类型为 Server 任务，则 Scope 必须为 Global，否则无效
    Write,
    // 删除 Crontab
    Delete,
}

// CrontabResult 权限枚举
// 注意：该权限仅在 Global Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResult {
    // 读取权限，指定可读取的 cron_name
    Read(String),
    // 删除权限，指定可删除的 cron_name
    Delete(String),
}

// Kv 权限枚举
// 注意：该权限仅在 Global 或 KvNamespace Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kv {
    // 列出该 Namespace 下所有键
    ListAllKeys,
    // 下面为 KV 数据库的 CRUD 操作
    // Write 在遇到同名 Key 会覆盖操作
    // 可以拥有通配符，比如 `metadata_*`，表达可以操作 这一 KvNamespace Scope 下的所有以 `metadata_` 开头的键
    Read(String),
    Write(String),
    Delete(String),
}

// Terminal 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    // 在 Agent Uuid 下拥有该权限，表明可以通过该 Token 连接到该 Agent 的 Terminal
    // Global Scope 下可以连接到所有的 Agent
    // 注意：此处只是连接，而不是创建或主动让 Agent 连接
    Connect,
}

// NodeGet 权限枚举
// 注意：该权限仅在 Global Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeGet {
    // 列出所有 Agent Uuid
    ListAllAgentUuid,
}

```

若存在于 Limit 的 permissions 中，即为拥有该权限

## Demo

### Agent 基础

现有这么一个结构体

```json
{
  "scopes": [
    {
      "agent_uuid": "adf78235-a23c-46fc-bc85-694f64c39aaf"
    },
    {
      "agent_uuid": "33c1b63a-35f1-4b9f-9659-66e7a3e5a75c"
    }
  ],
  "permissions": [
    {
      "dynamic_monitoring": "write"
    },
    {
      "static_monitoring": "write"
    },
    {
      "task": "listen"
    },
    {
      "task": {
        "write": "ping"
      }
    },
    {
      "task": {
        "write": "tcp_ping"
      }
    },
    {
      "task": {
        "write": "http_ping"
      }
    },
    {
      "task": {
        "write": "web_shell"
      }
    },
    {
      "task": {
        "write": "execute"
      }
    },
    {
      "task": {
        "write": "ip"
      }
    }
  ]
}
```

这是一个 Agent 能正常调用所有功能的 Limit，它表示:

Agent Uuid 为 `ad..af` 与 `33..5c` 的 Agent，具有上传 StaticMonitoring / DynamicMonitoring 数据、监听 Server 下发
Task、上报目前所有 Task 任务类型 的权限

### 查询 基础

现有这么一个结构体

```json
{
  "scopes": [
    {
      "agent_uuid": "53f125b6-e7aa-447f-a27c-085a53a36462"
    },
    {
      "agent_uuid": "3e6f227f-56e3-4ca0-a12f-04014ebeebe7"
    }
  ],
  "permissions": [
    {
      "dynamic_monitoring": {
        "read": "cpu"
      }
    },
    {
      "dynamic_monitoring": {
        "read": "system"
      }
    },
    {
      "static_monitoring": {
        "read": "cpu"
      }
    },
    {
      "static_monitoring": {
        "read": "system"
      }
    }
  ]
}
```

它表示:

用户可以查询 Agent Uuid 为 `ad..af` 与 `33..5c` 的 Agent 的 StaticMonitoring / DynamicMonitoring Data 中 cpu / system 字段

### Crontab 权限示例

现有这么一个结构体

```json
{
  "scopes": [
    {
      "global": null
    }
  ],
  "permissions": [
    {
      "crontab": "read"
    },
    {
      "crontab": "write"
    },
    {
      "crontab": "delete"
    }
  ]
}
```

这是一个具有全局 Crontab 权限的 Limit，它表示:

具有对所有 Crontab 的读取、写入和删除权限。

或针对特定 Agent 的权限:

```json
{
  "scopes": [
    {
      "agent_uuid": "00000000-0000-0000-0000-000000000001"
    },
    {
      "agent_uuid": "00000000-0000-0000-0000-000000000002"
    }
  ],
  "permissions": [
    {
      "crontab": "read"
    },
    {
      "crontab": "write"
    }
  ]
}
```

这表示:

对 UUID 为 `00000000-0000-0000-0000-000000000001` 和 `00000000-0000-0000-0000-000000000002` 的 Agent 相关的 Crontab
具有读取和写入权限。