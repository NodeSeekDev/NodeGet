# CrontabResult 总览

CrontabResult 是 Crontab 定时任务的执行结果记录，用于追踪每个定时任务的执行状态和结果

## 基本流程

当 Crontab 定时任务触发并执行完成后，执行结果会被记录到 CrontabResult 表中：

```
Crontab 触发 => 执行任务 => 记录执行结果 => 存储到数据库
```

调用者可以通过 JsonRpc API 查询和删除这些执行结果记录

## 数据结构

CrontabResult 结构如下:

```json
{
  "id": 1,
  // 记录 ID
  "cron_id": 5,
  // 关联的 Crontab ID
  "cron_name": "cleanup_database",
  // Crontab 名称
  "run_time": 1769341269012,
  // 执行时间（毫秒时间戳）
  "special_id": null,
  // 如果是下发 Agent Task 的 Cron，这里为 task_id
  // 如果是触发 JsWorker 的 Cron，这里为 js_result_id
  "success": true,
  // 是否执行成功
  "message": "Cleaned 100 records"
  // 执行结果消息
}
```

## 查询条件

需要用到统一的结构体 `CrontabResultQueryCondition`

其为 Rust Enum，解析时请注意:

```rust
#[serde(rename_all = "snake_case")]
pub enum CrontabResultQueryCondition {
    Id(i64),                      // 按记录 ID 过滤
    CronId(i64),                  // 按 cron_id 过滤
    CronName(String),             // 按 cron_name 过滤
    RunTimeFromTo(i64, i64),      // 按时间范围过滤（开始, 结束）
    RunTimeFrom(i64),             // 按起始时间过滤
    RunTimeTo(i64),               // 按结束时间过滤
    IsSuccess,                    // 仅查找成功的记录
    IsFailure,                    // 仅查找失败的记录
    Limit(u64),                   // 限制返回结果数量
    Last,                         // 获取最后一条记录
}
```

### 解析示例

```json
// 按 ID 查询
{
  "id": 1
}

// 按 cron_name 查询
{
  "cron_name": "cleanup_database"
}

// 按时间范围查询
{
  "run_time_from_to": [
    1700000000000,
    1800000000000
  ]
}

// 仅查询成功的记录
{
  "is_success": null
}

// 限制返回数量
{
  "limit": 100
}

// 获取最后一条记录
{
  "last": null
}
```

多个条件并存时，为 `AND`，即只查询满足所有条件的数据

## 权限说明

CrontabResult 的查询和删除权限仅在 `Global` Scope 下有效

权限结构示例:

```json
{
  "scopes": [
    "global"
  ],
  "permissions": [
    {
      "crontab_result": {
        "read": "cleanup_database"
      }
    },
    {
      "crontab_result": {
        "read": "backup_*"
      }
    },
    {
      "crontab_result": {
        "delete": "cleanup_database"
      }
    }
  ]
}
```

- `read`: 允许读取指定 cron_name 的结果记录，支持通配符 `*`
- `delete`: 允许删除指定 cron_name 的结果记录，支持通配符 `*`

注意: AgentUuid Scope 下的 CrontabResult 权限无效
