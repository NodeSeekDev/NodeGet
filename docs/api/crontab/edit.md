# 编辑 Crontab

编辑已存在的定时任务。

## 方法

调用方法名为 `crontab_edit`，需要提供以下参数:

```json
{
    "token": "demo_token",
    "name": "task_name",
    "cron_expression": "0 * * * * *",
    "cron_type": {
        // 任务类型，格式与 crontab_create 一致
    }
}
```

## 权限要求

编辑操作会做两层检查：

- 必须对目标 Crontab **原有内容** 的所有 Scope 拥有 `Crontab::Write`
- 必须对新提交的 `cron_type` 覆盖的所有 Scope 拥有写入权限（以及 Agent 类型所需的 `Task::Create`）

也就是说，只有完整覆盖相关 Scope 的 Token 才能编辑。

## 返回值

```json
{
    "id": 123,
    "success": true
}
```
