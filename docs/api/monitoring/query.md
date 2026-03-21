# 调用者查询

调用者可以通过 `agent_query_static(dynamic)` 查询

需要传入 `token` / `static(dynamic)_data_query`:

```json
{
    "token": "demo_token",
    "static(dynamic)_data_query": {
        "fields": [
            // DataQueryField 结构体，该结构体参考 Monitoring 总览
            // 该字段为 Vec<_>，可指定多个
        ],
        "condition": [
            // QueryCondition 结构体，该结构体参考 Monitoring 总览
            // 该字段为 Vec<_>，可指定多个
        ]
    }
}
```

返回结构:

```json
[
    // Monitoring 回报结构体，该结构体参考 Monitoring 总览
    // 该字段为 Vec<_>，可指定多个
    // 只会存在 DataQueryField 结构中指定的数据字段
]
```

## 分段平均查询

为了直接按时间段做聚合，新增两个方法：

- `agent_query_static_avg`
- `agent_query_dynamic_avg`

需要传入 `token` / `static(dynamic)_data_avg_query`:

```json
{
    "token": "demo_token",
    "static(dynamic)_data_avg_query": {
        "fields": [
            // DataQueryField 结构体，该结构体参考 Monitoring 总览
            // 该字段为 Vec<_>，可指定多个
        ],
        "uuid": "AGENT_UUID",
        "timestamp_from": 1,
        "timestamp_to": 2,
        "points": 100
    }
}
```

其中 `timestamp_from` / `timestamp_to` 可选，`points` 必须 >= 1。

语义说明：

1. 在筛选后的数据范围内（仅包含有数据的时间段）分成 `points` 份。
2. 每一份内对所选字段做平均值计算并返回。
3. 返回格式与 `agent_query_static(dynamic)` 一致：固定包含 `uuid` / `timestamp`，并包含 `fields` 指定字段。
4. `system` 字段仅保留 `process_count` 的平均值。
5. `disk` / `network` / `gpu` 字段中无法平均的子项将返回 `null`。

限制说明：

1. `agent_query_static_avg`
2. `agent_query_dynamic_avg`

这两个方法当前仅支持 PostgreSQL。

## 删除历史监控数据

新增两个删除方法：

- `agent_delete_static`
- `agent_delete_dynamic`

需要传入 `token` / `agent_uuid` / `before_timestamp`：

```json
{
    "token": "demo_token",
    "agent_uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3",
    "before_timestamp": 1769344168646
}
```

语义说明：

1. 仅删除 `timestamp < before_timestamp` 的数据。
2. 删除范围限定在指定 `agent_uuid`。
3. 返回值包含删除数量 `deleted`。

权限要求：

1. `agent_delete_static` 需要 `StaticMonitoring::Delete`。
2. `agent_delete_dynamic` 需要 `DynamicMonitoring::Delete`。

两者都要求 Token 在目标 `agent_uuid` Scope（或 Global Scope）下具备对应权限。

## 批量获取多个 Agent 的最新数据

为了便于直接查询多个 Agent 的最新一条监控数据，新增了两个方法：

- `agent_static_data_multi_last_query`
- `agent_dynamic_data_multi_last_query`

这两个方法等价于原 `agent_query_static(dynamic)` 中为每个 UUID 设置 `condition last` 的效果，但调用更直接。

需要传入 `token` / `uuids` / `fields`：

```json
{
    "token": "demo_token",
    "uuids": [
        "e8583352-39e8-5a5b-b66c-e450689088fd",
        "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
    ],
    "fields": [
        // DataQueryField 结构体，该结构体参考 Monitoring 总览
        // 该字段为 Vec<_>，可指定多个
    ]
}
```

返回结构:

```json
[
    // 每个 UUID 最多返回一条最新数据
    // 固定包含 uuid / timestamp
    // 只会包含 fields 指定的数据字段
]
```
