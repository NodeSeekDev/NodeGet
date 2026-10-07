---
outline: deep
---

# 查询、批量最新、删除

调用者通过以下方法查询和删除历史监控数据。关于查询条件和数据结构体的详细定义，请参考 [Monitoring 总览](./index.md)。

## Query Static

按条件查询静态监控数据。

### 方法

调用方法名为 `agent_query_static`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "static_data_query": {
    "fields": ["cpu", "system", "gpu"],
    "condition": [
      { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
      { "limit": 10 }
    ]
  }
}
```

参数结构体：

```rust
pub struct StaticDataQuery {
    pub fields: Vec<StaticDataQueryField>,  // 需要返回的字段
    pub condition: Vec<QueryCondition>,     // 查询条件
}
```

- `fields`: 指定返回哪些数据字段，可选值为 `cpu` / `system` / `gpu`。若为空，仅返回 `uuid` 和 `timestamp`，不返回任何数据字段
- `condition`: 查询条件列表，多个条件为 AND 关系。支持 `uuid` / `timestamp_from_to` / `timestamp_from` / `timestamp_to` /
  `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` / `last`

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大
> 10,000）。

### 权限要求

- **Scope**: 若 `condition` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `StaticMonitoring::Read(field)` — 当 `fields` 非空时，Token 必须对每个指定字段有 Read 权限；当 `fields`
  为空时，至少对一个字段有 Read 权限

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"static_monitoring": {"read": "cpu"}},
    {"static_monitoring": {"read": "system"}},
    {"static_monitoring": {"read": "gpu"}}
  ]
}
```

### 返回值

返回匹配记录的数组，每条记录固定包含 `uuid` 和 `timestamp`，其余字段按 `fields` 按需返回：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769341269012,
    "cpu": { ... },
    "system": { ... },
    "gpu": [ ... ]
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_query_static",
  "params": {
    "token": "demo_key:demo_secret",
    "static_data_query": {
      "fields": ["cpu", "system"],
      "condition": [
        { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
        "last"
      ]
    }
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769341269012,
      "cpu": {
        "physical_cores": 16,
        "logical_cores": 32,
        "per_core": [
          {
            "id": 1,
            "name": "CPU 1",
            "vendor_id": "AuthenticAMD",
            "brand": "AMD Ryzen 9 8945HX with Radeon Graphics"
          }
        ]
      },
      "system": {
        "system_name": "Windows",
        "system_kernel": "26200",
        "system_kernel_version": "Windows 11 IoT Enterprise LTSC 2024",
        "system_os_version": "11 (26200)",
        "system_os_long_version": "Windows 11 IoT Enterprise LTSC 2024",
        "distribution_id": "windows",
        "system_host_name": "DESKTOP-BI8T1T9",
        "arch": "x86_64",
        "virtualization": "HyperV"
      }
    }
  ]
}
```

## Query Dynamic

按条件查询动态监控数据。

### 方法

调用方法名为 `agent_query_dynamic`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "dynamic_data_query": {
    "fields": ["cpu", "ram", "network"],
    "condition": [
      { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
      { "limit": 10 }
    ]
  }
}
```

参数结构体：

```rust
pub struct DynamicDataQuery {
    pub fields: Vec<DynamicDataQueryField>,  // 需要返回的字段
    pub condition: Vec<QueryCondition>,      // 查询条件
}
```

- `fields`: 指定返回哪些数据字段，可选值为 `cpu` / `ram` / `load` / `system` / `disk` / `network` / `gpu`。若为空，仅返回
  `uuid` 和 `timestamp`，不返回任何数据字段
- `condition`: 查询条件列表，多个条件为 AND 关系。支持 `uuid` / `timestamp_from_to` / `timestamp_from` / `timestamp_to` /
  `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` / `last`

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大
> 10,000）。

### 权限要求

- **Scope**: 若 `condition` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `DynamicMonitoring::Read(field)` — 当 `fields` 非空时，Token 必须对每个指定字段有 Read 权限；当 `fields`
  为空时，至少对一个字段有 Read 权限

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"dynamic_monitoring": {"read": "cpu"}},
    {"dynamic_monitoring": {"read": "ram"}},
    {"dynamic_monitoring": {"read": "network"}}
  ]
}
```

### 返回值

返回匹配记录的数组，每条记录固定包含 `uuid` 和 `timestamp`，其余字段按 `fields` 按需返回：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769344168646,
    "cpu": { ... },
    "ram": { ... },
    "network": { ... }
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_query_dynamic",
  "params": {
    "token": "demo_key:demo_secret",
    "dynamic_data_query": {
      "fields": ["cpu", "ram"],
      "condition": [
        { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
        { "timestamp_from": 1769344160000 },
        { "limit": 5 }
      ]
    }
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769344168646,
      "cpu": {
        "per_core": [
          {
            "id": 1,
            "cpu_usage": 13.43,
            "frequency_mhz": 2007
          }
        ],
        "total_cpu_usage": 4.04
      },
      "ram": {
        "total_memory": 68501925888,
        "available_memory": 41439596544,
        "used_memory": 27062329344,
        "total_swap": 0,
        "used_swap": 0
      }
    }
  ]
}
```

## Query Traffic

查询一台设备在指定时间段内的流量。

> 周期流量 = 结束时刻的总流量快照 − 开始时刻的总流量快照。**总流量快照由 JS Worker 定时写入**（见
> [Write Traffic Snapshot](#write-traffic-snapshot)，NodeGet-Bootstrap 里的 `traffic-snapshot-worker` 就是做这件事的），
> 服务端自己不生成快照。没有安装并运行这个 Worker 时，没有快照数据，查不到任何时间段的流量。

### 方法

调用方法名为 `agent_query_traffic`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "query": {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "start_time": 1769344160000,
    "end_time": 1769347760000,
    "granularity": "total"
  }
}
```

参数结构体：

```rust
pub struct TrafficQuery {
    pub uuid: uuid::Uuid,               // 设备 UUID
    pub start_time: Option<i64>,        // 开始时间（毫秒），不填表示从最早开始
    pub end_time: Option<i64>,          // 结束时间（毫秒），不填表示到现在
    pub granularity: TrafficGranularity,
}

pub enum TrafficGranularity {
    Total,   // 只返回时间段内的流量合计
    Detail,  // 返回时间段内的每一条总流量快照
    Range,   // 返回每块网卡有快照数据的时间范围
}
```

- `uuid`: 要查询的设备 UUID
- `start_time` / `end_time`: 查询的时间范围（毫秒时间戳），任一不填表示不限制该端；`range` 会忽略这两个参数
- `granularity`:
  - `total`：按网卡返回时间段内的流量合计
  - `detail`：返回时间段内的每一条总流量快照，最长时间跨度 92 天
  - `range`：返回每块网卡最早和最晚的快照时间，用来判断从哪个时间起能查到数据

`total` 填了 `start_time` 时，每块网卡都必须有一条不晚于 `start_time` 的快照，否则返回 `NotFound`（没有开始时的总流量，不能当 0
算，否则会把开始之前的流量都算进去）。常见原因：

- 没有安装或没有运行流量统计 Worker
- 设备在 `start_time` 之后才开始统计（安装 Agent 之前的流量不会被统计）

遇到这种情况可以先用 `range` 查出每块网卡最早的快照时间，把 `start_time` 改到所有网卡都有数据之后再查。不填 `start_time` 时不受影响，
从开始统计起算。

流量统计只对**出口网卡**（真正连接外网的网卡）计数，容器、隧道等虚拟网卡不计入，避免重复统计。可以在 Agent 配置里通过
`dynamic_summary_select_network_interface` 手动指定要统计哪些网卡；不指定时按内置规则自动识别，详见
[Agent 配置](/guide/config/agent)。

### 权限要求

- **Scope**: `AgentUuid`，需覆盖 `query.uuid`
- **Permission**: `DynamicMonitoring::Read(Network)`

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"dynamic_monitoring": {"read": "network"}}
  ]
}
```

### 返回值

`granularity` 为 `total` 时，返回每块网卡的流量合计：

```json
{
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
  "start_time": 1769344160000,
  "end_time": 1769347760000,
  "interfaces": [
    { "interface_name": "eth0", "received": 1048576, "transmitted": 524288 }
  ],
  "received": 1048576,
  "transmitted": 524288,
  "possible_data_losses": []
}
```

`granularity` 为 `detail` 时，返回时间段内的每一条总流量快照：

```json
{
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
  "start_time": 1769344160000,
  "end_time": 1769347760000,
  "snapshots": [
    {
      "interface_name": "eth0",
      "snapshot_time": 1769344200000,
      "total_received": 10485760,
      "total_transmitted": 5242880
    }
  ],
  "possible_data_losses": []
}
```

`total` 和 `detail` 的返回都带有 `possible_data_losses`：与查询时间段有重叠的"可能丢失数据"时间段（比如网卡计数器重置、设备离线超过 10
分钟），提醒这段时间内的流量统计可能不准：

```json
[
  { "start_time": 1769344000000, "end_time": 1769344500000 }
]
```

`granularity` 为 `range` 时，返回每块网卡有快照数据的时间范围，按网卡名排序；设备还没有任何快照时 `interfaces` 为空数组。没有
`possible_data_losses`：

```json
{
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
  "interfaces": [
    {
      "interface_name": "eth0",
      "first_snapshot_time": 1769000000000,
      "last_snapshot_time": 1769344200000
    }
  ]
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_query_traffic",
  "params": {
    "token": "demo_key:demo_secret",
    "query": {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "start_time": 1769344160000,
      "end_time": 1769347760000,
      "granularity": "total"
    }
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "start_time": 1769344160000,
    "end_time": 1769347760000,
    "interfaces": [
      { "interface_name": "eth0", "received": 1048576, "transmitted": 524288 }
    ],
    "received": 1048576,
    "transmitted": 524288,
    "possible_data_losses": []
  }
}
```

## Query Traffic Current

查询设备当前的总流量（出口网卡跨重启累加的总量）。数据来自服务端内存，不经过快照，所以是最新的。同时返回每块网卡最晚的快照时间，供写快照的
Worker 判断某个时间段是否已经存过快照。

### 方法

调用方法名为 `agent_query_traffic_current`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "uuids": ["e8583352-39e8-5a5b-b66c-e450689088fd"]
}
```

- `uuids`：要查询的设备 UUID 列表，自动去重；**不填表示所有设备**；传空数组 `[]` 直接返回 `[]`

### 权限要求

- **Scope**:
  - 指定了 `uuids`：需要覆盖每一个 UUID 的 `AgentUuid`
  - 没有指定 `uuids`（所有设备）：需要 `Global`，只有某几台设备权限的 Token 不能查所有设备
- **Permission**: `DynamicMonitoring::Read(Network)`

权限配置示例（所有设备）：

```json
{
  "scopes": [
    {"global": null}
  ],
  "permissions": [
    {"dynamic_monitoring": {"read": "network"}}
  ]
}
```

### 返回值

按设备 UUID、网卡名排序的数组。已删除的设备、不存在的 UUID、没有出口网卡的设备都不会出现在结果里：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "interfaces": [
      {
        "interface_name": "eth0",
        "total_received": 10485760,
        "total_transmitted": 5242880,
        "updated_at": 1769344250000,
        "last_snapshot_time": 1769344200000
      }
    ]
  }
]
```

- `total_received` / `total_transmitted`：到现在为止的总接收量 / 总发送量（字节）
- `updated_at`：总流量最近一次更新的时间（毫秒）
- `last_snapshot_time`：已存快照中最晚的时间（毫秒），还没有快照时为 `null`

## Write Traffic Snapshot

批量写入总流量快照。快照由 JS Worker 定时写入：Worker 用 [Query Traffic Current](#query-traffic-current)
取出当前总流量，到了该存快照的时间点就调用这个接口落库。服务端只负责校验和存储。

### 方法

调用方法名为 `agent_write_traffic_snapshot`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "snapshots": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "interface_name": "eth0",
      "snapshot_time": 1769344200000,
      "total_received": 10485760,
      "total_transmitted": 5242880
    }
  ]
}
```

- `snapshot_time`：快照时间（毫秒）
- `total_received` / `total_transmitted`：到该时刻为止的总流量（字节），不能是负数

数据校验，任何一条不合法整个请求失败（返回 `InvalidInput`），一条也不会写入：

- 一次最多 10000 条
- `interface_name` 为 1 到 255 个字符
- `snapshot_time` 不能小于 0，也不能比服务端当前时间晚超过 1 分钟（时间写成未来，会让这块网卡之后的快照都被认为"已经存过"而不再写）
- 总流量不能是负数

设备不存在或已删除的条目会被跳过，不报错，计入返回值的 `skipped`。同一设备、同一网卡、同一 `snapshot_time` 已有快照时忽略（不覆盖旧值），所以重复写入是安全的。整个请求在一个事务里完成。

### 权限要求

- **Scope**: `AgentUuid`，需覆盖请求里的每一台设备
- **Permission**: `DynamicMonitoring::Write`

权限配置示例：

```json
{
  "scopes": [
    {"global": null}
  ],
  "permissions": [
    {"dynamic_monitoring": "write"}
  ]
}
```

### 返回值

```json
{
  "inserted": 1,
  "ignored": 0,
  "skipped": 0
}
```

- `inserted`：新写入的条数
- `ignored`：因为同一网卡同一时间已有快照而忽略的条数
- `skipped`：因为设备不存在或已删除而跳过的条数

## Delete Traffic Snapshot

删除指定时间及之前的总流量快照，同时清理结束时间不晚于该时间的"可能丢失数据"时间段（跨过该时间的保留）。供 Worker 定时清理过期数据。

### 方法

调用方法名为 `agent_delete_traffic_snapshot`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "end_time": 1769344200000,
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"
}
```

- `end_time`：必填，删除 `snapshot_time` 小于等于它的快照（毫秒）
- `uuid`：只清理这台设备；**不填表示所有设备**（包括已软删除的设备）。指定的 UUID 不存在时返回 `NotFound`

快照是分批删除的，每批最多 5000 条，每批是独立的一条 SQL，不放在同一个事务里。第一次清理大量历史数据时不会长时间占着 SQLite 的写锁；中途失败时已删除的批次不会回滚。

### 权限要求

- **Scope**:
  - 指定了 `uuid`：`AgentUuid`，需覆盖该 UUID
  - 没有指定 `uuid`（所有设备）：需要 `Global`
- **Permission**: `DynamicMonitoring::Delete`

权限配置示例：

```json
{
  "scopes": [
    {"global": null}
  ],
  "permissions": [
    {"dynamic_monitoring": "delete"}
  ]
}
```

### 返回值

```json
{
  "deleted_snapshots": 1200,
  "deleted_possible_data_losses": 2
}
```

## Static Data Multi Last Query

批量获取多个 Agent 的最新一条静态监控数据。等价于为每个 UUID 执行 `agent_query_static` 并设置 `condition: ["last"]`。

### 方法

调用方法名为 `agent_static_data_multi_last_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "uuids": [
    "e8583352-39e8-5a5b-b66c-e450689088fd",
    "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
  ],
  "fields": ["cpu", "system"]
}
```

参数说明：

- `token`: Token
- `uuids`: Agent UUID 列表。若为空数组，直接返回 `[]`
- `fields`: 需要返回的字段，可选值为 `cpu` / `system` / `gpu`

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖 `uuids` 中的每一个 UUID
- **Permission**: `StaticMonitoring::Read(field)` — 当 `fields` 非空时，Token 必须对每个指定字段有 Read 权限；当 `fields`
  为空时，至少对一个字段有 Read 权限

### 返回值

返回数组，每个 UUID 最多一条最新记录：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769341269012,
    "cpu": { ... },
    "system": { ... }
  },
  {
    "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3",
    "timestamp": 1769341200000,
    "cpu": { ... },
    "system": { ... }
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_static_data_multi_last_query",
  "params": {
    "token": "demo_key:demo_secret",
    "uuids": [
      "e8583352-39e8-5a5b-b66c-e450689088fd",
      "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
    ],
    "fields": ["cpu", "system"]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769341269012,
      "cpu": {
        "physical_cores": 16,
        "logical_cores": 32,
        "per_core": [
          {
            "id": 1,
            "name": "CPU 1",
            "vendor_id": "AuthenticAMD",
            "brand": "AMD Ryzen 9 8945HX with Radeon Graphics"
          }
        ]
      },
      "system": {
        "system_name": "Windows",
        "system_kernel": "26200",
        "system_kernel_version": "Windows 11 IoT Enterprise LTSC 2024",
        "system_os_version": "11 (26200)",
        "system_os_long_version": "Windows 11 IoT Enterprise LTSC 2024",
        "distribution_id": "windows",
        "system_host_name": "DESKTOP-BI8T1T9",
        "arch": "x86_64",
        "virtualization": "HyperV"
      }
    },
    {
      "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3",
      "timestamp": 1769341200000,
      "cpu": {
        "physical_cores": 8,
        "logical_cores": 16,
        "per_core": [
          {
            "id": 1,
            "name": "CPU 1",
            "vendor_id": "GenuineIntel",
            "brand": "Intel Core i7-13700K"
          }
        ]
      },
      "system": {
        "system_name": "Linux",
        "system_kernel": "6.8.0",
        "system_kernel_version": "6.8.0-generic",
        "system_os_version": "24.04",
        "system_os_long_version": "Ubuntu 24.04 LTS",
        "distribution_id": "ubuntu",
        "system_host_name": "server-01",
        "arch": "x86_64",
        "virtualization": ""
      }
    }
  ]
}
```

## Dynamic Data Multi Last Query

批量获取多个 Agent 的最新一条动态监控数据。等价于为每个 UUID 执行 `agent_query_dynamic` 并设置 `condition: ["last"]`。

### 方法

调用方法名为 `agent_dynamic_data_multi_last_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "uuids": [
    "e8583352-39e8-5a5b-b66c-e450689088fd",
    "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
  ],
  "fields": ["cpu", "ram", "network"]
}
```

参数说明：

- `token`: Token
- `uuids`: Agent UUID 列表。若为空数组，直接返回 `[]`
- `fields`: 需要返回的字段，可选值为 `cpu` / `ram` / `load` / `system` / `disk` / `network` / `gpu`

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖 `uuids` 中的每一个 UUID
- **Permission**: `DynamicMonitoring::Read(field)` — 当 `fields` 非空时，Token 必须对每个指定字段有 Read 权限；当 `fields`
  为空时，至少对一个字段有 Read 权限

### 返回值

返回数组，每个 UUID 最多一条最新记录：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769344168646,
    "cpu": { ... },
    "ram": { ... },
    "network": { ... }
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_dynamic_data_multi_last_query",
  "params": {
    "token": "demo_key:demo_secret",
    "uuids": [
      "e8583352-39e8-5a5b-b66c-e450689088fd"
    ],
    "fields": ["cpu", "ram"]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769344168646,
      "cpu": {
        "per_core": [
          {
            "id": 1,
            "cpu_usage": 13.43,
            "frequency_mhz": 2007
          }
        ],
        "total_cpu_usage": 4.04
      },
      "ram": {
        "total_memory": 68501925888,
        "available_memory": 41439596544,
        "used_memory": 27062329344,
        "total_swap": 0,
        "used_swap": 0
      }
    }
  ]
}
```

## Delete Static

删除历史静态监控数据。

### 方法

调用方法名为 `agent_delete_static`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "conditions": [
    { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
    { "timestamp_to": 1769344168646 }
  ]
}
```

参数说明：

- `token`: Token
- `conditions`: `Vec<QueryCondition>` — 使用与查询相同的条件结构体（支持 `uuid` / `timestamp_from_to` /
  `timestamp_from` / `timestamp_to` / `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` /
  `last`）。删除语义与查询语义一致

注意事项：

- 若包含 `last` / `limit`，会按时间倒序选中对应记录后删除
- 多个条件为 AND 关系
- `limit` 单次最多删除 10,000 条（与查询路径一致，超过会被钳制；如需清理更多数据，请按时间范围分批删除或省略 `limit` 走全量删除路径）

### 权限要求

- **Scope**: 若 `conditions` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `StaticMonitoring::Delete`

### 返回值

删除成功后返回：

```json
{
  "success": true,
  "deleted": 42,
  "condition_count": 2
}
```

- `deleted`: 实际删除的记录数
- `condition_count`: 使用的条件数量

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_delete_static",
  "params": {
    "token": "demo_key:demo_secret",
    "conditions": [
      { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
      { "timestamp_to": 1769344168646 }
    ]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "deleted": 42,
    "condition_count": 2
  }
}
```

## Delete Dynamic

删除历史动态监控数据。

### 方法

调用方法名为 `agent_delete_dynamic`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "conditions": [
    { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
    { "timestamp_to": 1769344168646 }
  ]
}
```

参数说明：

- `token`: Token
- `conditions`: `Vec<QueryCondition>` — 使用与查询相同的条件结构体（支持 `uuid` / `timestamp_from_to` /
  `timestamp_from` / `timestamp_to` / `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` /
  `last`）。删除语义与查询语义一致

注意事项：

- 若包含 `last` / `limit`，会按时间倒序选中对应记录后删除
- 多个条件为 AND 关系
- `limit` 单次最多删除 10,000 条（与查询路径一致，超过会被钳制；如需清理更多数据，请按时间范围分批删除或省略 `limit` 走全量删除路径）

### 权限要求

- **Scope**: 若 `conditions` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `DynamicMonitoring::Delete`

### 返回值

删除成功后返回：

```json
{
  "success": true,
  "deleted": 1500,
  "condition_count": 2
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_delete_dynamic",
  "params": {
    "token": "demo_key:demo_secret",
    "conditions": [
      { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
      { "timestamp_to": 1769344168646 }
    ]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "deleted": 1500,
    "condition_count": 2
  }
}
```

## Query Dynamic Summary

按条件查询动态摘要监控数据。

### 方法

调用方法名为 `agent_query_dynamic_summary`，需要提供以下参数：

```json
{
  "token": "demo_token",
    "query": {
    "fields": ["cpu_usage", "used_memory", "total_memory"],
    "condition": [
      { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
      { "limit": 10 }
    ]
  }
}
```

参数结构体：

```rust
pub struct DynamicSummaryQuery {
    pub fields: Vec<DynamicSummaryQueryField>,  // 需要返回的字段
    pub condition: Vec<QueryCondition>,         // 查询条件
}
```

- `fields`: 指定返回哪些数据字段，可选值为 `cpu_usage` / `gpu_usage` / `used_swap` / `total_swap` / `used_memory` /
  `total_memory` / `available_memory` / `load_one` / `load_five` / `load_fifteen` / `uptime` / `boot_time` /
  `process_count` / `total_space` / `available_space` / `read_speed` / `write_speed` / `tcp_connections` /
  `udp_connections` / `total_received` / `total_transmitted` / `transmit_speed` / `receive_speed`。若为空，返回所有字段
- `condition`: 查询条件列表，多个条件为 AND 关系。支持 `uuid` / `timestamp_from_to` / `timestamp_from` / `timestamp_to` /
  `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` / `last`

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大
> 10,000）。

### 权限要求

- **Scope**: 若 `condition` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `DynamicMonitoringSummary::Read`

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"dynamic_monitoring_summary": "read"}
  ]
}
```

### 返回值

返回匹配记录的数组，每条记录固定包含 `uuid` 和 `timestamp`，其余字段按 `fields` 按需返回：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769344168646,
    "cpu_usage": 4.0,
    "used_memory": 27062329344,
    "total_memory": 68501925888
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_query_dynamic_summary",
  "params": {
    "token": "demo_key:demo_secret",
  "query": {
      "fields": ["cpu_usage", "used_memory", "total_memory"],
      "condition": [
        { "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" },
        { "timestamp_from": 1769344160000 },
        { "limit": 5 }
      ]
    }
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769344168646,
      "cpu_usage": 4.0,
      "used_memory": 27062329344,
      "total_memory": 68501925888
    }
  ]
}
```

## Dynamic Summary Multi Last Query

批量获取多个 Agent 的最新一条动态摘要监控数据。等价于为每个 UUID 执行 `agent_query_dynamic_summary` 并设置
`condition: ["last"]`。

### 方法

调用方法名为 `agent_dynamic_summary_multi_last_query`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "uuids": [
    "e8583352-39e8-5a5b-b66c-e450689088fd",
    "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
  ],
  "fields": ["cpu_usage", "used_memory", "total_memory"]
}
```

参数说明：

- `token`: Token
- `uuids`: Agent UUID 列表。若为空数组，直接返回 `[]`
- `fields`: 需要返回的字段，可选值同 `DynamicSummaryQueryField`

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖 `uuids` 中的每一个 UUID
- **Permission**: `DynamicMonitoringSummary::Read`

### 返回值

返回数组，每个 UUID 最多一条最新记录：

```json
[
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "timestamp": 1769344168646,
    "cpu_usage": 4.0,
    "used_memory": 27062329344,
    "total_memory": 68501925888
  }
]
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_dynamic_summary_multi_last_query",
  "params": {
    "token": "demo_key:demo_secret",
    "uuids": [
      "e8583352-39e8-5a5b-b66c-e450689088fd"
    ],
    "fields": ["cpu_usage", "used_memory"]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "timestamp": 1769344168646,
      "cpu_usage": 4.0,
      "used_memory": 27062329344
    }
  ]
}
```

## Delete Dynamic Summary

删除历史动态摘要监控数据。

### 方法

调用方法名为 `agent_delete_dynamic_summary`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "conditions": [
    { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
    { "timestamp_to": 1769344168646 }
  ]
}
```

参数说明：

- `token`: Token
- `conditions`: `Vec<QueryCondition>` — 使用与查询相同的条件结构体（支持 `uuid` / `timestamp_from_to` /
  `timestamp_from` / `timestamp_to` / `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` /
  `last`）。删除语义与查询语义一致

注意事项：

- 若包含 `last` / `limit`，会按时间倒序选中对应记录后删除
- 多个条件为 AND 关系
- `limit` 单次最多删除 10,000 条（与查询路径一致，超过会被钳制；如需清理更多数据，请按时间范围分批删除或省略 `limit` 走全量删除路径）

### 权限要求

- **Scope**: 若 `conditions` 中包含 `uuid`，需覆盖对应的 `AgentUuid`；若不包含 `uuid`，需要 `Global` Scope
- **Permission**: `DynamicMonitoringSummary::Delete`

### 返回值

删除成功后返回：

```json
{
  "success": true,
  "deleted": 1500,
  "condition_count": 2
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_delete_dynamic_summary",
  "params": {
    "token": "demo_key:demo_secret",
    "conditions": [
      { "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3" },
      { "timestamp_to": 1769344168646 }
    ]
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "deleted": 1500,
    "condition_count": 2
  }
}
```
