# Crontab CRUD

## Create Crontab

创建新的 Crontab 定时任务。

### 方法

调用方法名为 `crontab_create`，需要提供以下参数：

```json
{
  "token": "demo_token",           // Token
  "name": "task_name",             // 任务名称
  "cron_expression": "0 0 * * * *",  // Cron 表达式（秒 分 时 日 月 周）
  "cron_type": {
    // 任务类型，详情见下文
  }
}
```

该方法仅用于 **创建**。如果 `name` 已存在，会直接返回错误，不会覆盖原有 Crontab。

#### Cron 表达式

Cron 表达式遵循标准格式，包含秒、分、时、日、月、周字段。

例如：

- `0 * * * * *` 表示每分钟执行一次
- `0 0 * * * *` 表示每小时执行一次

#### Cron 类型

Cron 任务支持两种类型：

Agent 任务类型 — 在特定 Agent 上执行任务:

```json
{
  "agent": [
    [
      "00000000-0000-0000-0000-000000000001", // Agent UUID
      "00000000-0000-0000-0000-000000000002"
    ],
    {
      "task": {
        "ping": "www.example.com" // TaskEventType
      }
    }
  ]
}
```

Server 任务类型 — 在服务器上执行数据库清理任务:

```json
{
  "server": "clean_up_database"
}
```

Server 任务类型 — 触发已注册的 JsWorker 脚本:

```json
{
  "server": {
    "js_worker": [
      "demo_nodeget_fetch",  // 脚本名（js_worker.name）
      {
        "hello": "from_cron" // 传给脚本的 params（任意 JSON）
      }
    ]
  }
}
```

说明：

- 第一个参数是脚本名（`js_worker.name`）
- 第二个参数是传给脚本的 `params`（任意 JSON）
- Cron 触发时不传 `env`，会使用脚本自身在数据库保存的 `env`
- 触发成功后会生成 `js_result` 记录，`crontab_result.relative_id` 即该 `js_result.id`

### 权限要求

创建 Crontab 需要：

- `Crontab::Write`
- 若是 Agent 类型，还需要对应任务类型的 `Task::Create`
- 若是 `server.js_worker` 类型，还需要 `JsWorker::RunDefinedJsWorker`（作用域需覆盖该脚本名）

并且必须覆盖 `cron_type` 中声明的 **所有 Scope**（例如 Agent 列表中的每个 UUID）。

示例权限配置:

```json
{
  "scopes": [
    {"agent_uuid": "00000000-0000-0000-0000-000000000001"},
    {"agent_uuid": "00000000-0000-0000-0000-000000000002"}
  ],
  "permissions": [
    {"crontab": "write"},
    {"task": {"create": "ping"}},
    {"task": {"create": "tcp_ping"}}
  ]
}
```

也可以使用通配符一次性授权多种任务类型：

```json
{
  "scopes": [
    {"agent_uuid": "00000000-0000-0000-0000-000000000001"}
  ],
  "permissions": [
    {"crontab": "write"},
    {"task": {"create": "*"}},      // 授权创建所有任务类型
    {"task": {"write": "tcp*"}}     // 授权写入所有 tcp 开头的任务
  ]
}
```

`server.js_worker` 权限示例：

```json
{
  "scopes": [
    {"global": null},
    {"js_worker": "demo_*"}
  ],
  "permissions": [
    {"crontab": "write"},
    {"js_worker": "run_defined_js_worker"}
  ]
}
```

### 返回值

创建成功后返回任务 ID:

```json
{
  "id": 123 // 数据库中的任务 ID
}
```

### 完整示例

请求（Agent 类型）:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_create",
  "params": {
    "token": "demo_token",
    "name": "ping_task",
    "cron_expression": "0 * * * * *",
    "cron_type": {
      "agent": [
        [
          "00000000-0000-0000-0000-000000000001",
          "00000000-0000-0000-0000-000000000002"
        ],
        {
          "task": {
            "ping": "www.example.com"
          }
        }
      ]
    }
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": 123
  }
}
```

请求 (Server JsWorker 类型):

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_create",
  "params": {
    "token": "demo_token",
    "name": "cron_js_demo",
    "cron_expression": "*/5 * * * * *",
    "cron_type": {
      "server": {
        "js_worker": [
          "demo_nodeget_fetch",
          {
            "hello": "from_cron"
          }
        ]
      }
    }
  },
  "id": 2
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "id": 124
  }
}
```

## Edit Crontab

编辑已存在的 Crontab 定时任务。

### 方法

调用方法名为 `crontab_edit`，需要提供以下参数：

```json
{
  "token": "demo_token",                // Token
  "name": "task_name",                  // 目标任务名称
  "cron_expression": "0 * * * * *",     // 新的 Cron 表达式
  "cron_type": {
    // 新的任务类型，格式与 crontab_create 一致
  }
}
```

### 权限要求

编辑操作会做两层检查：

- 必须对目标 Crontab **原有内容** 的所有 Scope 拥有 `Crontab::Write`
- 必须对新提交的 `cron_type` 覆盖的所有 Scope 拥有写入权限（以及 Agent 类型所需的 `Task::Create`）
- 若新类型为 `server.js_worker`，还必须拥有目标脚本的 `JsWorker::RunDefinedJsWorker` 权限

也就是说，只有完整覆盖相关 Scope 的 Token 才能编辑。

### 返回值

编辑成功后返回:

```json
{
  "id": 123,       // 任务 ID
  "success": true  // 操作结果
}
```

### 完整示例

请求（Agent 类型）:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_edit",
  "params": {
    "token": "demo_token",
    "name": "ping_task",
    "cron_expression": "0 */5 * * * *",
    "cron_type": {
      "agent": [
        [
          "00000000-0000-0000-0000-000000000001"
        ],
        {
          "task": {
            "tcp_ping": "www.example.com:443"
          }
        }
      ]
    }
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": 123,
    "success": true
  }
}
```

请求 (Server JsWorker 类型):

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_edit",
  "params": {
    "token": "demo_token",
    "name": "cron_js_demo",
    "cron_expression": "*/5 * * * * *",
    "cron_type": {
      "server": {
        "js_worker": [
          "demo_nodeget_fetch",
          {
            "hello": "from_edit"
          }
        ]
      }
    }
  },
  "id": 2
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "id": 124,
    "success": true
  }
}
```

## Get Crontab

获取当前 Token 可见的所有 Crontab 定时任务。

### 方法

调用方法名为 `crontab_get`，需要提供以下参数：

```json
{
  "token": "demo_token" // Token
}
```

### 权限要求

读取 Crontab 需要 `Crontab::Read` 权限。

根据 Token 的作用域限制，返回的 Crontab 列表会有所不同：

- **Global 权限**: 返回所有 Crontab（包括 Agent 和 Server 类型）
- **AgentUuid 权限**: 只返回与指定 Agent UUID 相关的 Agent 类型 Crontab

### 返回值

读取成功后返回 Crontab 列表，每个元素结构如下:

```json
[
  {
    "id": 123,                              // 任务 ID
    "name": "ping_task",                    // 任务名称
    "enable": true,                         // 是否启用
    "cron_expression": "0 * * * * *",       // Cron 表达式
    "cron_type": {                          // 任务类型
      "agent": [
        [
          "00000000-0000-0000-0000-000000000001"
        ],
        {
          "task": {
            "ping": "www.example.com"
          }
        }
      ]
    },
    "last_run_time": 1769341269012          // 最后运行时间（毫秒时间戳），可为 null
  }
]
```

若数据库中存在损坏的 `cron_type` 数据，接口会直接返回解析错误（包含对应 Crontab 的 `id` 和 `name`），不会再静默回退为默认
`Server` 任务类型。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_get",
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
  "id": 1,
  "result": [
    {
      "id": 123,
      "name": "ping_task",
      "enable": true,
      "cron_expression": "0 * * * * *",
      "cron_type": {
        "agent": [
          [
            "00000000-0000-0000-0000-000000000001"
          ],
          {
            "task": {
              "ping": "www.example.com"
            }
          }
        ]
      },
      "last_run_time": 1769341269012
    },
    {
      "id": 124,
      "name": "db_cleanup",
      "enable": true,
      "cron_expression": "0 0 3 * * *",
      "cron_type": {
        "server": "clean_up_database"
      },
      "last_run_time": 1769337198000
    }
  ]
}
```

## Delete Crontab

删除指定的 Crontab 定时任务。

### 方法

调用方法名为 `crontab_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",             // Token
  "name": "task_name_to_delete"      // 要删除的任务名称
}
```

### 权限要求

删除 Crontab 需要 `Crontab::Delete` 权限。

服务端会先读取目标 Crontab，并要求该 Token 对其 `cron_type` 对应的所有 Scope 均具备删除权限。

例如，若目标 Crontab 为 Agent 类型且包含两个 UUID，则 Token 必须在这两个 UUID 的 Scope 下均拥有 `Crontab::Delete`。

### 返回值

删除成功后返回:

```json
{
  "success": true // 操作结果
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_delete",
  "params": {
    "token": "demo_token",
    "name": "ping_task"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true
  }
}
```

## Set Enable

强制设置指定 Crontab 定时任务的启用/禁用状态。

### 方法

调用方法名为 `crontab_set_enable`，需要提供以下参数：

```json
{
  "token": "demo_token",    // Token
  "name": "task_name",      // 目标任务名称
  "enable": true            // 启用状态，true 为启用，false 为禁用
}
```

此操作会将任务的状态强制设置为指定的启用/禁用状态：

- `enable: true` 将任务设置为启用
- `enable: false` 将任务设置为禁用

### 权限要求

设置 Crontab 启用状态需要 `Crontab::Write` 权限。

服务端会先读取目标 Crontab，并要求该 Token 对其 `cron_type` 对应的所有 Scope 均具备写权限。

### 返回值

设置成功后返回:

```json
{
  "success": true,  // 操作结果
  "enabled": true   // 当前的启用状态
}
```

### 完整示例

请求（启用任务）:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_set_enable",
  "params": {
    "token": "demo_token",
    "name": "ping_task",
    "enable": true
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "enabled": true
  }
}
```

请求（禁用任务）:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab_set_enable",
  "params": {
    "token": "demo_token",
    "name": "ping_task",
    "enable": false
  },
  "id": 2
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "success": true,
    "enabled": false
  }
}
```
