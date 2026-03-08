# 设置 Crontab 启用状态

强制设置指定定时任务的启用/禁用状态。

## 方法

调用方法名为 `crontab_set_enable`，需要提供以下参数:

```json
{
    "token": "demo_token",
    "name": "task_name",
    "enable": true
}
```

此操作会将任务的状态强制设置为指定的启用/禁用状态：

- `enable: true` 将任务设置为启用
- `enable: false` 将任务设置为禁用

## 权限要求

设置 Crontab 启用状态需要 `Crontab::Write` 权限。

服务端会先读取目标 Crontab，并要求该 Token 对其 `cron_type` 对应的所有 Scope 均具备写权限。
