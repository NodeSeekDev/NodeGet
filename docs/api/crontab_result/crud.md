# CrontabResult CRUD

## Query CrontabResult

调用者可以通过 `crontab-result_query` 查询执行结果记录。

### 方法

调用方法名为 `crontab-result_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "query": {
    "condition": [
      // CrontabResultQueryCondition 结构体，该结构体参考 CrontabResult 总览
      // 该字段为 Vec<_>，可指定多个
    ]
  }
}
```

### 权限要求

- 需要 `CrontabResult::Read(String)` 权限
- 仅在 `Global` Scope 下有效
- `read` 权限中的字符串对应 `cron_name`，支持通配符 `*`

### 返回值

```json
[
  {
    "id": 1,                             // 记录 ID
    "cron_id": 5,                        // 关联的 Crontab ID
    "cron_name": "cleanup_database",     // Crontab 名称
    "special_id": null,                  // 关联的 task_id 或 js_result_id
    "run_time": 1769341269012,           // 执行时间（毫秒时间戳）
    "success": true,                     // 是否执行成功
    "message": "Cleaned 100 records"     // 执行结果消息
  }
  // 该字段为 Vec<_>，可返回多条
]
```

### 完整示例

查询指定 cron_name 的所有结果:

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "cron_name": "cleanup_database"
        }
      ]
    }
  },
  "id": 1
}
```

查询指定时间范围内的成功记录:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "cron_name": "cleanup_database"
        },
        {
          "run_time_from_to": [1700000000000, 1800000000000]
        },
        {
          "is_success": null
        }
      ]
    }
  },
  "id": 1
}
```

查询最近的 10 条失败记录:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "cron_name": "backup_database"
        },
        {
          "is_failure": null
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

获取最后一条执行记录:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "cron_name": "cleanup_database"
        },
        "last"
      ]
    }
  },
  "id": 1
}
```

注意: 需要分别查询每个 cron_name，不支持一次查询多个不同的 cron_name。

## Delete CrontabResult

调用者可以通过 `crontab-result_delete` 删除执行结果记录。

### 方法

调用方法名为 `crontab-result_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "delete_params": {
    "cron_name": "cleanup_database", // 可选，若指定则只删除该 cron_name 的记录
    "before_time": 1700000000000     // 删除该时间之前的记录（毫秒时间戳）
  }
}
```

### 权限要求

删除操作需要 `CrontabResult::Delete(String)` 权限，仅在 `Global` Scope 下有效。

权限结构示例:

```json
{
  "scopes": [
    "global"
  ],
  "permissions": [
    {
      "crontab_result": {
        "delete": "cleanup_database" // 删除指定 cron_name
      }
    },
    {
      "crontab_result": {
        "delete": "backup_*" // 删除匹配通配符的 cron_name
      }
    },
    {
      "crontab_result": {
        "delete": "*" // 删除所有（全局权限）
      }
    }
  ]
}
```

- 若指定了 `cron_name`，则检查对该 cron_name 的删除权限
- 若未指定 `cron_name`（删除所有），则需要全局删除权限 `{"delete": "*"}`

### 返回值

```json
{
  "success": true,       // 是否成功
  "deleted_count": 100   // 删除的记录数量
}
```

### 完整示例

删除指定 cron_name 在指定时间之前的所有记录:

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_delete",
  "params": {
    "token": "demo_token",
    "delete_params": {
      "cron_name": "cleanup_database",
      "before_time": 1700000000000
    }
  },
  "id": 1
}
```

删除所有记录（需要全局删除权限）:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_delete",
  "params": {
    "token": "demo_token",
    "delete_params": {
      "before_time": 1700000000000
    }
  },
  "id": 1
}
```
