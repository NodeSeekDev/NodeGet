# 删除 Crontab

删除指定的定时任务。

## 方法

调用方法名为 `crontab_delete`，需要提供以下参数:

```json
{
    "token": "demo_token",
    "name": "task_name_to_delete"
}
```

## 权限要求

删除 Crontab 需要 `Crontab::Delete` 权限。

服务端会先读取目标 Crontab，并要求该 Token 对其 `cron_type` 对应的所有 Scope 均具备删除权限。
