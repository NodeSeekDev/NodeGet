# 调用者查询

调用者可以通过 `crontab-result_query` 查询

注意：该字符串是完整 JSON-RPC 方法名（`namespace = crontab-result` + `method = query`）。

需要传入 `token` / `query`:

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

返回结构:

```json
[
  {
    "id": 1,
    "cron_id": 5,
    "cron_name": "cleanup_database",
    "run_time": 1769341269012,
    "success": true,
    "message": "Cleaned 100 records"
  }
  // 该字段为 Vec<_>，可指定多个
]
```

## 完整示例

### 查询指定 cron_name 的所有结果

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": [
    "demo_token",
    {
      "condition": [
        {
          "cron_name": "cleanup_database"
        }
      ]
    }
  ],
  "id": 1
}
```

### 查询指定时间范围内的成功记录

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": [
    "demo_token",
    {
      "condition": [
        {
          "cron_name": "cleanup_database"
        },
        {
          "run_time_from_to": [
            1700000000000,
            1800000000000
          ]
        },
        {
          "is_success": null
        }
      ]
    }
  ],
  "id": 1
}
```

### 查询最近的 10 条失败记录

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": [
    "demo_token",
    {
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
  ],
  "id": 1
}
```

### 获取最后一条执行记录

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": [
    "demo_token",
    {
      "condition": [
        {
          "cron_name": "cleanup_database"
        },
        {
          "last": null
        }
      ]
    }
  ],
  "id": 1
}
```

### 使用通配符权限查询多个 cron_name

如果令牌具有 `{"crontab_result": {"read": "cleanup_*"}}` 权限，可以查询所有以 `cleanup_` 开头的任务:

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_query",
  "params": [
    "demo_token",
    {
      "condition": [
        {
          "cron_name": "cleanup_database"
        },
        {
          "limit": 50
        }
      ]
    }
  ],
  "id": 1
}
```

注意: 需要分别查询每个 cron_name，不支持一次查询多个不同的 cron_name
