# JsResult 总览

`JsResult` 是 `js-worker_run` 的异步执行结果表。

一次 `run` 会先创建一条结果记录并返回 `id`，脚本执行完成后再回填该记录。

## 方法列表

| 方法名                                        | 描述     |
|----------------------------------------------|--------|
| [query](./crud.md#query-jsresult)   | 查询执行结果 |
| [delete](./crud.md#delete-jsresult) | 删除执行结果 |

## 数据结构

`JsResult` 每条记录包含：

```json
{
  "id": 1, // 记录 ID
  "js_worker_id": 10, // 关联的 JsWorker ID
  "js_worker_name": "demo_worker", // 关联的 JsWorker 名称
  "run_type": "call", // 执行类型
  "start_time": 1775000000000, // 毫秒时间戳，开始时间
  "finish_time": 1775000000123, // 毫秒时间戳，结束时间，运行中为 null
  "param": { // 执行参数，可为 null
    "hello": "world"
  },
  "result": { // 执行结果，运行中或失败时为 null
    "ok": true
  },
  "error_message": null // 错误信息，成功或运行中时为 null
}
```

### 注意事项

- `result` 与 `error_message` 至少有一个会被回填。
- 运行中状态定义为：`finish_time == null && result == null && error_message == null`。

## 查询条件

统一使用 `JsResultQueryCondition`，其为 Rust Enum，解析时请注意：

```rust
#[serde(rename_all = "snake_case")]
pub enum JsResultQueryCondition {
    Id(i64),
    JsWorkerId(i64),
    JsWorkerName(String),
    RunType(String),
    StartTimeFromTo(i64, i64),
    StartTimeFrom(i64),
    StartTimeTo(i64),
    FinishTimeFromTo(i64, i64),
    FinishTimeFrom(i64),
    FinishTimeTo(i64),
    IsSuccess,
    IsFailure,
    IsRunning,
    Limit(u64),
    Last,
}
```

下面是一些解析的示例：

```json
{
    "id": 1
}

{
    "js_worker_name": "demo_worker"
}

{
    "start_time_from_to": [1775000000000, 1775000001000]
}

{
    "start_time_from": 1775000000000
}

{
    "limit": 100 // 依照 start_time 最新的 100 条
}

"last" // 对就是一个 `last`，无其他东西

"is_success" // 同理，无其他东西

"is_failure"

"is_running"
```

### 注意事项

`start_time_from_to` 字段可看作是 `start_time_from` 与 `start_time_to` 的简略写法，下面的两种表达方式是等价的：

```json
{
    "start_time_from_to": [1775000000000, 1775000001000]
}

[
    {
        "start_time_from": 1775000000000
    },
    {
        "start_time_to": 1775000001000
    }
]
```

`finish_time_from_to` 同理。

`limit` 为 1 与 `last` 等价，在数据库层面限制查询结果，按照时间倒序排列。

多个条件并存时，为 `AND`，即只查询满足所有条件的数据。

## 权限说明

`JsResult` 权限基于 `Scope::JsWorker(String)` 生效，支持后缀 `*` 通配符。

```json
{
  "scopes": [
    {
      "js_worker": "demo_*"
    }
  ],
  "permissions": [
    {
      "js_result": {
        "read": "demo_*"
      }
    },
    {
      "js_result": {
        "delete": "demo_*"
      }
    }
  ]
}
```

- `read`：可查询匹配脚本名的结果。
- `delete`：可删除匹配脚本名的结果。
