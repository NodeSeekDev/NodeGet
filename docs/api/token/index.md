# Token 总览

Token 是本项目的鉴权核心，任何有权限的操作都应持有 有对应权限的 Token

## 方法列表

| 方法名                                                | 描述          |
|----------------------------------------------------|-------------|
| [token_get](./crud.md#get-token)                   | 获取 Token 信息 |
| [token_create](./crud.md#create-token)             | 创建新 Token   |
| [token_delete](./crud.md#delete-token)             | 删除 Token    |
| [token_edit](./crud.md#edit-token)                 | 编辑 Token    |
| [token_list_all_tokens](./crud.md#list-all-tokens) | 列出所有 Token  |

## Token 分类

在本项目，Token 可以分为两类

- SuperToken: 在 Server 初始化时创建的唯一值，数据库 ID 为 1 的 Token，在所有操作中该 Token 直接放行
- Token: 由 SuperToken 创建的子 Token

安全约束:

- SuperToken 作为根鉴权记录（`id = 1`）不可被删除。

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
    pub version: i32,              // 暂时为 1
    pub token_key: String,         // 标识 Token 最主要的键
    pub timestamp_from: Option<i64>, // Token 有效期，毫秒时间戳
    pub timestamp_to: Option<i64>,
    pub token_limit: Vec<Limit>,   // 权限范围
    pub username: Option<String>,  // 用户名
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

Scope 为作用域，即表示在某一个对象上有权限（Agent / Kv Namespace / JsWorker）

```rust
pub enum Scope {
    Global,                    // 全局作用域，适用于所有地点
    AgentUuid(uuid::Uuid),     // 特定 Agent 作用域，通过 UUID 指定
    KvNamespace(String),       // KvNamespace 作用域，通过名称指定
    JsWorker(String),          // JsWorker 作用域，通过脚本名指定，支持后缀 * 通配
}
```

### Permission

```rust
// 权限枚举，定义不同类型的操作权限
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    StaticMonitoring(StaticMonitoring),   // 静态监控权限
    DynamicMonitoring(DynamicMonitoring), // 动态监控权限
    Task(Task),                           // 任务权限
    Crontab(Crontab),                     // Crontab 权限
    CrontabResult(CrontabResult),         // CrontabResult 权限
    Kv(Kv),                               // Kv 权限
    Terminal(Terminal),                   // Terminal 权限
    NodeGet(NodeGet),                     // NodeGet 权限
    JsWorker(JsWorker),                   // Js Worker 权限
    JsResult(JsResult),                   // Js Result 权限
}

// 静态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticMonitoring {
    Read(StaticDataQueryField), // 读取权限，指定可读取的字段类型
    Write,                      // 写入权限
    Delete,                     // 删除权限
}

// 动态监控权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicMonitoring {
    Read(DynamicDataQueryField), // 读取权限，指定可读取的字段类型
    Write,                       // 写入权限
    Delete,                      // 删除权限
}

// 任务权限枚举
// Type 字段名
// 接受 ping / tcp_ping / http_ping / web_shell / execute / http_request / ip
// 支持通配符 `*`：
// - `"*"` 匹配所有任务类型
// - `"tcp*"` 匹配以 tcp 开头的任务类型（如 tcp_ping）
// - 仅支持后缀通配符，不支持 `*ping` 或 `t*p`
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Create(String), // 创建权限，指定任务类型，支持通配符
    Read(String),   // 读取权限，指定任务类型，支持通配符
    Write(String),  // 写入权限，指定任务类型，支持通配符
    Delete(String), // 删除权限，指定任务类型，支持通配符
    Listen,         // 监听权限
}

// Crontab 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crontab {
    Read,   // 可以读取在自己 Scope 下的所有 Crontab
    Write,  // 可以创建 Crontab
            // 若 Crontab 类型为下发给 Agent 任务，则该 Token 还必须拥有对应 Agent 的 Task Create 权限
            // 若 Crontab 类型为 Server 任务，则 Scope 必须为 Global，否则无效
    Delete, // 删除 Crontab
}

// CrontabResult 权限枚举
// 注意：该权限仅在 Global Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrontabResult {
    Read(String),   // 读取权限，指定可读取的 cron_name
    Delete(String), // 删除权限，指定可删除的 cron_name
}

// Kv 权限枚举
// 注意：该权限仅在 Global 或 KvNamespace Scope 下有效
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kv {
    ListAllNamespace, // 列出可见 Namespace
    ListAllKeys,      // 列出该 Namespace 下所有键
    Read(String),     // 读取 KV 数据，支持通配符如 `metadata_*`
    Write(String),    // 写入 KV 数据，遇到同名 Key 会覆盖
    Delete(String),   // 删除 KV 数据
}

// Terminal 权限枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    Connect, // 在 Agent Uuid 下拥有该权限，表明可以通过该 Token 连接到该 Agent 的 Terminal
             // Global Scope 下可以连接到所有的 Agent
             // 注意：此处只是连接，而不是创建或主动让 Agent 连接
}

// NodeGet 权限枚举
// 在 Global Scope 下可列出系统内全部 Agent UUID
// 在 AgentUuid Scope 下可列出对应范围内的 Agent UUID（仍需方法层校验）
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeGet {
    ListAllAgentUuid, // 列出所有 Agent Uuid
    GetRtPool,        // 查看 JS Runtime 池信息
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsWorker {
    ListAllJsWorker,    // 列出当前 Token 在权限范围内且数据库真实存在的脚本
    Create,              // 创建 JsWorker
    Read,                // 读取 JsWorker
    Write,               // 更新 JsWorker
    Delete,              // 删除 JsWorker
    RunDefinedJsWorker,  // 运行已定义的 JsWorker
    RunRawJsWorker,      // 运行原始 JsWorker
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsResult {
    Read(String),   // 读取权限，支持后缀 * 通配
    Delete(String), // 删除权限，支持后缀 * 通配
}
```

若存在于 Limit 的 permissions 中，即为拥有该权限

### 注意事项

`JsWorker` 与 `JsResult` 都使用 `Scope::JsWorker(String)` 进行范围约束:

- `Scope::JsWorker("demo_worker")`：仅作用于 `demo_worker`
- `Scope::JsWorker("demo_*")`：作用于所有以 `demo_` 开头的脚本

`JsResult` 的 `Read/Delete(String)` 也支持同样的后缀 `*` 通配

Monitoring 删除权限:

- `StaticMonitoring::Delete`：允许调用 `agent_delete_static`
- `DynamicMonitoring::Delete`：允许调用 `agent_delete_dynamic`

两者均需配合目标 Agent 的 Scope（`AgentUuid`）使用，或在 `Global` Scope 下全局生效

若需要调用 `js-worker_get_rt_pool`，应授予 `Permission::NodeGet(NodeGet::GetRtPool)`，建议配合 `Scope::Global`

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
      "dynamic_monitoring": "write" // 上报动态监控数据
    },
    {
      "static_monitoring": "write" // 上报静态监控数据
    },
    {
      "task": "listen" // 监听 Server 下发 Task
    },
    {
      "task": {
        "write": "ping" // 上报 ping 任务
      }
    },
    {
      "task": {
        "write": "tcp_ping" // 上报 tcp_ping 任务
      }
    },
    {
      "task": {
        "write": "http_ping" // 上报 http_ping 任务
      }
    },
    {
      "task": {
        "write": "web_shell" // 上报 web_shell 任务
      }
    },
    {
      "task": {
        "write": "execute" // 上报 execute 任务
      }
    },
    {
      "task": {
        "write": "ip" // 上报 ip 任务
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
        "read": "cpu" // 读取动态 CPU 数据
      }
    },
    {
      "dynamic_monitoring": {
        "read": "system" // 读取动态 System 数据
      }
    },
    {
      "static_monitoring": {
        "read": "cpu" // 读取静态 CPU 数据
      }
    },
    {
      "static_monitoring": {
        "read": "system" // 读取静态 System 数据
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
      "global": null // 全局作用域
    }
  ],
  "permissions": [
    {
      "crontab": "read" // 读取 Crontab
    },
    {
      "crontab": "write" // 创建 Crontab
    },
    {
      "crontab": "delete" // 删除 Crontab
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
      "crontab": "read" // 读取 Crontab
    },
    {
      "crontab": "write" // 创建 Crontab
    }
  ]
}
```

这表示:

对 UUID 为 `00000000-0000-0000-0000-000000000001` 和 `00000000-0000-0000-0000-000000000002` 的 Agent 相关的 Crontab
具有读取和写入权限。

### JsWorker 权限示例

现有这么一个结构体：

```json
{
  "scopes": [
    {
      "js_worker": "demo_*" // 匹配所有 demo_ 前缀的脚本
    }
  ],
  "permissions": [
    {
      "js_worker": "list_all_js_worker" // 列出匹配的 JsWorker
    },
    {
      "js_worker": "create" // 创建 JsWorker
    },
    {
      "js_worker": "read" // 读取 JsWorker
    },
    {
      "js_worker": "write" // 更新 JsWorker
    },
    {
      "js_worker": "delete" // 删除 JsWorker
    },
    {
      "js_worker": "run_defined_js_worker" // 运行已定义的 JsWorker
    },
    {
      "js_result": {
        "read": "demo_*" // 读取 demo_ 前缀脚本的执行结果
      }
    },
    {
      "js_result": {
        "delete": "demo_*" // 删除 demo_ 前缀脚本的执行结果
      }
    }
  ]
}
```

它表示：

- 允许操作所有 `demo_` 前缀的 JsWorker
- 允许查询/删除所有 `demo_` 前缀脚本的执行结果
