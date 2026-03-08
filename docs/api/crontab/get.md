# 读取 Crontab

获取定时任务列表。

## 方法

调用方法名为 `crontab_get`，需要提供以下参数:

```json
{
    "token": "demo_token"
}
```

## 权限要求

读取 Crontab 需要 `Crontab::Read` 权限。

根据 Token 的作用域限制，返回的 Crontab 列表会有所不同：

- **Global 权限**: 返回所有 Crontab（包括 Agent 和 Server 类型）
- **AgentUuid 权限**: 只返回与指定 Agent UUID 相关的 Agent 类型 Crontab

## 返回值

读取成功后返回 Crontab 列表

## 错误语义

若数据库中存在损坏的 `cron_type` 数据，接口会直接返回解析错误（包含对应 Crontab 的 `id` 和 `name`），不会再静默回退为默认 `Server` 任务类型。
