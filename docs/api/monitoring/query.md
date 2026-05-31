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

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大 10,000）。

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

- `fields`: 指定返回哪些数据字段，可选值为 `cpu` / `ram` / `load` / `system` / `disk` / `network` / `gpu`。若为空，仅返回 `uuid` 和 `timestamp`，不返回任何数据字段
- `condition`: 查询条件列表，多个条件为 AND 关系。支持 `uuid` / `timestamp_from_to` / `timestamp_from` / `timestamp_to` /
  `storage_time_from_to` / `storage_time_from` / `storage_time_to` / `limit` / `last`

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大 10,000）。

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

> **默认 LIMIT**：若 `condition` 中未指定 `limit` 或 `last`，查询默认限制返回 10,000 条记录。显式指定 `limit` 可覆盖此默认值（最大 10,000）。

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
