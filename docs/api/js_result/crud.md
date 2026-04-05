# JsResult CRUD

## Query JsResult

调用者可以通过 `js-result_query` 查询 JsResult 执行结果。

### 方法

调用方法名为 `js-result_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "query": {
    "condition": [
      // JsResultQueryCondition 结构体，该结构体参考 JsResult 总览
      // 该字段为 Vec<_>，可指定多个
    ]
  }
}
```

### 权限要求

- Permission: `JsResult::Read("worker_name_or_pattern")`
- Scope: `JsWorker(worker_name)`，支持后缀 `*` 通配

### 返回值

```json
[
  {
    "id": 1,                              // 记录 ID
    "js_worker_id": 10,                   // 关联的 JsWorker ID
    "js_worker_name": "demo_worker",      // 关联的 JsWorker 名称
    "run_type": "call",                   // 执行类型
    "start_time": 1775000000000,          // 毫秒时间戳，开始时间
    "finish_time": 1775000000123,         // 毫秒时间戳，结束时间，运行中为 null
    "param": { "hello": "world" },        // 执行参数，可为 null
    "result": { "ok": true },             // 执行结果，运行中或失败时为 null
    "error_message": null                 // 错误信息，成功或运行中时为 null
  }
  // 该字段为 Vec<_>，可返回多条
]
```

### 完整示例

查询某个脚本最近 10 条结果:

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
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

查询运行中的记录:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        "is_running"
      ]
    }
  },
  "id": 2
}
```

查询最后一条记录:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        "last"
      ]
    }
  },
  "id": 3
}
```

按 run_type 查询:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_query",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        {
          "run_type": "inline_call"
        },
        {
          "limit": 20
        }
      ]
    }
  },
  "id": 4
}
```

## Delete JsResult

调用者可以通过 `js-result_delete` 删除 JsResult 执行结果。

### 方法

调用方法名为 `js-result_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "query": {
    "condition": [
      // JsResultQueryCondition 结构体，该结构体参考 JsResult 总览
      // 该字段为 Vec<_>，可指定多个
    ]
  }
}
```

说明：

- 删除语义与查询一致：`condition` 能查到什么，就删除什么。
- 当包含 `last` 或 `limit` 时，会先按 `start_time DESC, id DESC` 选中目标行再删除。

### 权限要求

- Permission: `JsResult::Delete("worker_name_or_pattern")`
- Scope: `JsWorker(worker_name)`，支持后缀 `*` 通配

### 返回值

```json
{
  "success": true,        // 是否成功
  "deleted": 8,           // 删除的记录数
  "condition_count": 3    // 条件数量
}
```

### 完整示例

删除某脚本最后一条结果:

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_delete",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        "last"
      ]
    }
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "deleted": 1,
    "condition_count": 2
  }
}
```

删除某脚本最近 50 条失败结果:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_delete",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        "is_failure",
        {
          "limit": 50
        }
      ]
    }
  },
  "id": 2
}
```

按 run_type 删除:

```json
{
  "jsonrpc": "2.0",
  "method": "js-result_delete",
  "params": {
    "token": "demo_token",
    "query": {
      "condition": [
        {
          "js_worker_name": "demo_worker"
        },
        {
          "run_type": "inline_call"
        },
        {
          "limit": 20
        }
      ]
    }
  },
  "id": 3
}
```
