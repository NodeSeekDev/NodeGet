# 调用者删除

调用者可以通过 `crontab-result_delete` 删除记录

注意：该字符串是完整 JSON-RPC 方法名（`namespace = crontab-result` + `method = delete`）。

需要传入 `token` / `delete_params`:

```json
{
  "token": "demo_token",
  "delete_params": {
    "cron_name": "cleanup_database",
    // 可选，若指定则只删除该 cron_name 的记录
    "before_time": 1700000000000
    // 删除该时间之前的记录（毫秒时间戳）
  }
}
```

返回结构:

```json
{
  "success": true,
  "deleted_count": 100
}
```

## 完整示例

### 删除指定 cron_name 在指定时间之前的所有记录

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_delete",
  "params": [
    "demo_token",
    {
      "cron_name": "cleanup_database",
      "before_time": 1700000000000
    }
  ],
  "id": 1
}
```

### 删除所有记录（需要全局删除权限）

```json
{
  "jsonrpc": "2.0",
  "method": "crontab-result_delete",
  "params": [
    "demo_token",
    {
      "before_time": 1700000000000
    }
  ],
  "id": 1
}
```

## 权限说明

删除操作需要 `crontab_result` 下的 `delete` 权限:

```json
{
  "scopes": [
    "global"
  ],
  "permissions": [
    {
      "crontab_result": {
        "delete": "cleanup_database"
      }
    },
    // 删除指定 cron_name
    {
      "crontab_result": {
        "delete": "backup_*"
      }
    },
    // 删除匹配通配符的 cron_name
    {
      "crontab_result": {
        "delete": "*"
      }
    }
    // 删除所有（全局权限）
  ]
}
```

注意:

- 若指定了 `cron_name`，则检查对该 cron_name 的删除权限
- 若未指定 `cron_name`（删除所有），则需要全局删除权限 `{"delete": "*"}`
