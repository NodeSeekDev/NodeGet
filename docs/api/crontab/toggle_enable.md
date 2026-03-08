# 切换 Crontab 启用状态

切换指定定时任务的启用/禁用状态。

## 方法

调用方法名为 `crontab_toggle_enable`，需要提供以下参数:

```json
{
    "token": "demo_token",
    "name": "task_name_to_toggle"
}
```

此操作会将当前的启用状态切换为相反状态：

- 如果任务当前是启用的，则切换为禁用
- 如果任务当前是禁用的，则切换为启用

## 权限要求

切换 Crontab 启用状态需要 `Crontab::Write` 权限。

服务端会先读取目标 Crontab，并要求该 Token 对其 `cron_type` 对应的所有 Scope 均具备写权限。
