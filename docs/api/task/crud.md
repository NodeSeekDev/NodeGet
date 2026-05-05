# Task CRUD

## Create Task

调用者可以通过 `task_create_task` 给指定 Agent 下发 Task。

### 方法

调用方法名为 `task_create_task`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "target_uuid": "AGENT_UUID_HERE", // 指定的 Agent UUID
  "task_type": {
    // 任务主体，该结构体参考 Task 总览
  }
}
```

语义说明：

1. `target_uuid` 为目标 Agent 的 UUID，该 Agent 必须已连接并注册任务订阅。
2. `task_type` 结构体参考 Task 总览中的 `TaskEventType`。
3. 当 `task_type` 为 `web_shell` 时，必须携带 `terminal_id`（随机 UUID）。
4. 当 `task_type` 为 `execute` 时，必须使用结构化参数（`cmd + args`），且 `cmd` 不能为空字符串。
5. 当 `task_type` 为 `http_request` 时，`body` 与 `body_base64` 互斥，最多只能出现一个；`ip` 可传具体 IP，或 `"ipv4 auto"` /
   `"ipv6 auto"`。

### 权限要求

需要 Token 具备目标 Agent UUID Scope（或 Global Scope）下的 Task 创建权限。

### 返回值

```json
{
  "id": 4 // 数据库中的 ID 字段，可通过该字段作为条件查询
}
```

### 完整示例

请求 (ping):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "ping": "www.example.com"
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
    "id": 4
  }
}
```

请求 (web_shell):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "web_shell": {
        "url": "wss://YOUR_SERVER/auto_gen",                   // WebSocket URL
        "terminal_id": "4c8d1cba-244e-4baf-9b65-c881f86ca60a" // 随机 UUID
      }
    }
  },
  "id": 1
}
```

请求 (execute):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "execute": {
        "cmd": "ls",       // 命令名，不能为空字符串
        "args": ["-1", "tmp"] // 参数列表
      }
    }
  },
  "id": 1
}
```

如需 shell 语法，请显式调用 shell 程序并传参（示例：`bash -c` 或 `cmd /C`），而不是直接传一整段 shell 字符串。

请求 (http_request):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "http_request": {
        "url": "https://example.com",              // 完整 URL
        "method": "POST",                          // HTTP 方法
        "headers": {                               // 请求头
          "content-type": "application/json"
        },
        "body": "{\"hello\":\"world\"}",           // 与 body_base64 互斥
        "ip": "ipv4 auto"                          // 指定出口 IP，可选
      }
    }
  },
  "id": 1
}
```

请求 (self_update):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "self_update": "v0.0.14"    // 目标版本号，格式 vX.Y.Z，支持升级和降级
    }
  },
  "id": 1
}
```

SelfUpdate 任务触发 Agent 从 `https://install.nodeget.com/` 下载对应架构的二进制并自动替换。
需确保 Agent 配置中 `allow_self_update = true`，且版本号格式为 `vX.Y.Z`（仅做格式校验，不做版本大小比较，支持升级和降级）。
- Unix 平台：使用 execv 替换当前进程（不创建新进程）
- Windows 平台：拉起新进程后自身退出

错误示例 — Agent 未注册:

```json
{
  "error_id": 104,
  "error_message": "Error sending task event: Agent AGENT_UUID_HERE is not connected"
}
```

## Create Task Blocking

`task_create_task_blocking` 是 `task_create_task` 的阻塞版本。创建任务后不立即返回 ID，而是等待 Agent
执行完毕并上传结果后，将完整的任务结果直接返回给调用者。

### 方法

调用方法名为 `task_create_task_blocking`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "target_uuid": "AGENT_UUID_HERE",
  "task_type": {
    // 任务主体，与 task_create_task 完全一致
  },
  "timeout_ms": 5000 // 超时时间（毫秒）
}
```

语义说明：

1. `token`、`target_uuid`、`task_type` 的含义与 `task_create_task` 完全一致。
2. `timeout_ms` 为等待 Agent 返回结果的最大时间（毫秒）。超时后返回错误，但任务本身仍然存在于数据库中（Agent 后续仍可上传结果）。
3. 内部流程：创建任务 → 发送给 Agent → 等待 Agent 上传结果 → 返回完整结果。

### 权限要求

需要 Token 同时具备目标 Agent UUID Scope（或 Global Scope）下的 Task 创建权限和 Task 读取权限。

### 返回值

成功时返回完整的 `TaskEventResponse`：

```json
{
  "task_id": 4,
  "agent_uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82",
  "task_token": "aBcDeFgHiJ",
  "timestamp": 1769341269012,
  "success": true,
  "error_message": null,
  "task_event_result": {
    "ping": 12.5
  }
}
```

超时时返回错误：

```json
{
  "error": {
    "code": 999,
    "message": "Task 4 timed out after 5000ms"
  }
}
```

### 完整示例

请求（ping，超时 5 秒）:

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task_blocking",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "ping": "www.example.com"
    },
    "timeout_ms": 5000
  },
  "id": 1
}
```

响应（Agent 在超时前返回了结果）:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "task_id": 4,
    "agent_uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82",
    "task_token": "aBcDeFgHiJ",
    "timestamp": 1769341269012,
    "success": true,
    "error_message": null,
    "task_event_result": {
      "ping": 12.5
    }
  }
}
```

请求 (execute，超时 30 秒):

```json
{
  "jsonrpc": "2.0",
  "method": "task_create_task_blocking",
  "params": {
    "token": "demo_token",
    "target_uuid": "AGENT_UUID_HERE",
    "task_type": {
      "execute": {
        "cmd": "uname",
        "args": ["-a"]
      }
    },
    "timeout_ms": 30000
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
    "task_id": 5,
    "agent_uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82",
    "task_token": "xYzAbCdEfG",
    "timestamp": 1769341270000,
    "success": true,
    "error_message": null,
    "task_event_result": {
      "execute": "Linux server 6.1.0 #1 SMP x86_64 GNU/Linux\n"
    }
  }
}
```

## Query Task

调用者可以通过 `task_query` 查询任务记录。

### 方法

调用方法名为 `task_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "task_data_query": {
    "condition": [
      // TaskQueryCondition 结构体，该结构体参考 Task 总览
      // 该字段为 Vec<_>，可指定多个
    ]
  }
}
```

语义说明：

1. `condition` 使用 Task 总览中的 `TaskQueryCondition` 结构体。
2. `cron_source` 为可选字段：若该任务由 crontab 创建，则为对应的 cron `name`；否则为 `null`。
3. 多个条件并存时为 `AND`，即只返回满足所有条件的记录。

### 权限要求

需要 Token 具备对应 Agent UUID Scope（或 Global Scope）下的 Task 查询权限。

### 返回值

```json
[
  {
    "cron_source": "daily_check",       // 若由 crontab 创建则为 cron name，否则为 null
    "error_message": null,              // 若 success 为 false 则包含错误信息
    "success": true,                    // 是否成功
    "task_event_result": {
      // 任务回报结构体，该结构体参考 Task 总览
    },
    "task_event_type": {
      // 任务主体，该结构体参考 Task 总览
    },
    "task_id": 6,                       // 数据库中的 ID 字段
    "timestamp": 1769341269012,         // 毫秒时间戳
    "uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82" // Agent UUID
  }
  // 该字段为 Vec<_>，可返回多条
]
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "task_query",
  "params": {
    "token": "demo_token",
    "task_data_query": {
      "condition": [
        {
          "uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82"
        },
        {
          "type": "ping"
        },
        {
          "limit": 10
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
  "result": [
    {
      "cron_source": null,
      "error_message": null,
      "success": true,
      "task_event_result": {
        "ping": 12.5
      },
      "task_event_type": {
        "ping": "www.example.com"
      },
      "task_id": 6,
      "timestamp": 1769341269012,
      "uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82"
    }
  ]
}
```

## Delete Task

调用者可以通过 `task_delete` 删除任务记录。

### 方法

调用方法名为 `task_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "conditions": [
    // TaskQueryCondition 结构体，参考 Task 总览
    // 查询能命中的记录，就是删除会影响的记录
  ]
}
```

语义说明：

1. `conditions` 使用与 `task_query` 完全一致的 `TaskQueryCondition`。
2. 删除语义与查询语义一致：查询能命中的记录，就是删除会影响的记录。
3. 若包含 `last` / `limit`，会按 `timestamp desc, id desc` 先选中再删除。
4. 若不含 `last` / `limit`，则按过滤条件批量删除。

### 权限要求

- 需要 `Task::Delete(String)` 权限。
- 当 `conditions` 包含 `type` 时，需要对应类型的删除权限。
- 当 `conditions` 不包含 `type` 时，要求覆盖所有任务类型的删除权限。

### 返回值

```json
{
  "success": true,        // 是否成功
  "deleted": 12,          // 删除的记录数
  "condition_count": 2    // 条件数量
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "task_delete",
  "params": {
    "token": "demo_token",
    "conditions": [
      {
        "uuid": "42e89a61-39de-4569-b6ef-e86bc3ed8f82"
      },
      {
        "type": "ping"
      },
      {
        "limit": 100
      }
    ]
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
    "deleted": 12,
    "condition_count": 3
  }
}
```
