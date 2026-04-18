---
outline: deep
---

# Agent 上报

Agent 通过以下方法将采集的监控数据上报至 Server。关于上报数据结构体的详细定义，请参考 [Monitoring 总览](./index.md)。

## Report Static

上报 Agent 的静态监控数据（CPU 型号、系统信息、GPU 信息等）。

### 方法

调用方法名为 `agent_report_static`，需要提供以下参数：

```json
{
  "token": "demo_token",                   // Token
  "static_monitoring_data": {              // StaticMonitoringData 结构体
    // 完整结构体参考 Monitoring 总览
  }
}
```

参数说明：

- `token`: 具有上报权限的 Token（格式: `token_key:token_secret`）
- `static_monitoring_data`: StaticMonitoringData 结构体，包含 `uuid`、`time`、`data_hash`、`cpu`、`system`、`gpu` 字段

也支持元组方式传参：

```json
{
  "params": [
    "demo_token",
    {
      // StaticMonitoringData 结构体
    }
  ]
}
```

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖上报数据中的 `uuid`
- **Permission**: `StaticMonitoring::Write`

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"static_monitoring": "write"}
  ]
}
```

### 返回值

上报成功后返回缓冲状态。数据会先缓存在内存中，由后台定时批量写入数据库：

```json
{
  "status": "buffered"
}
```

若该 Agent 已存在相同 `data_hash` 的记录（数据内容未变化），则跳过写入：

```json
{
  "status": "skipped",
  "reason": "duplicate_hash"
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_report_static",
  "params": {
    "token": "demo_key:demo_secret",
    "static_monitoring_data": {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "time": 1769341269012,
      "data_hash": [171, 205, 18, 52, 86, 120, 144, 171, 205, 239, 1, 35, 69, 103, 137, 171],
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
      },
      "gpu": [
        {
          "id": 1,
          "name": "NVIDIA GeForce RTX 5060 Laptop GPU",
          "cuda_cores": 3328,
          "architecture": "Blackwell"
        }
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
  "result": {
    "status": "buffered"
  }
}
```

## Report Dynamic

上报 Agent 的动态监控数据（CPU 使用率、内存、磁盘、网络、GPU 状态等）。

### 方法

调用方法名为 `agent_report_dynamic`，需要提供以下参数：

```json
{
  "token": "demo_token",                    // Token
  "dynamic_monitoring_data": {              // DynamicMonitoringData 结构体
    // 完整结构体参考 Monitoring 总览
  }
}
```

参数说明：

- `token`: 具有上报权限的 Token（格式: `token_key:token_secret`）
- `dynamic_monitoring_data`: DynamicMonitoringData 结构体，包含 `uuid`、`time`、`cpu`、`ram`、`load`、`system`、`disk`、
  `network`、`gpu` 字段

也支持元组方式传参：

```json
{
  "params": [
    "demo_token",
    {
      // DynamicMonitoringData 结构体
    }
  ]
}
```

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖上报数据中的 `uuid`
- **Permission**: `DynamicMonitoring::Write`

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"dynamic_monitoring": "write"}
  ]
}
```

### 返回值

上报成功后返回缓冲状态。数据会先缓存在内存中，由后台定时批量写入数据库：

```json
{
  "status": "buffered"
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_report_dynamic",
  "params": {
    "token": "demo_key:demo_secret",
    "dynamic_monitoring_data": {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "time": 1769344168646,
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
      },
      "load": {
        "one": 0,
        "five": 0,
        "fifteen": 0
      },
      "system": {
        "boot_time": 1769337198,
        "uptime": 6970,
        "process_count": 313
      },
      "disk": [
        {
          "kind": "Ssd",
          "name": "",
          "file_system": "NTFS",
          "mount_point": "C:\\",
          "total_space": 322057531392,
          "available_space": 91563786240,
          "is_removable": false,
          "is_read_only": false,
          "read_speed": 35741,
          "write_speed": 49550
        }
      ],
      "network": {
        "interfaces": [
          {
            "interface_name": "WLAN",
            "total_received": 527863209,
            "total_transmitted": 484144450,
            "receive_speed": 5559,
            "transmit_speed": 1626
          }
        ],
        "udp_connections": 67,
        "tcp_connections": 165
      },
      "gpu": [
        {
          "id": 1,
          "used_memory": 2169692160,
          "total_memory": 8546942976,
          "graphics_clock_mhz": 510,
          "sm_clock_mhz": 510,
          "memory_clock_mhz": 405,
          "video_clock_mhz": 622,
          "utilization_gpu": 5,
          "utilization_memory": 30,
          "temperature": 51
        }
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
  "result": {
    "status": "buffered"
  }
}
```

## Report Dynamic Summary

上报 Agent 的动态摘要监控数据。与 DynamicMonitoringData 不同，摘要数据将关键指标扁平化为独立列，所有字段（除 `uuid` 和 `time`）均为可选。

### 方法

调用方法名为 `agent_report_dynamic_summary`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "dynamic_monitoring_summary_data": {
    // DynamicMonitoringSummaryData 结构体
  }
}
```

参数说明：

- `token`: 具有上报权限的 Token（格式: `token_key:token_secret`）
- `dynamic_monitoring_summary_data`: DynamicMonitoringSummaryData 结构体，包含 `uuid`、`time` 及各可选指标字段

也支持元组方式传参：

```json
{
  "params": [
    "demo_token",
    {
      // DynamicMonitoringSummaryData 结构体
    }
  ]
}
```

### 权限要求

- **Scope**: `AgentUuid` — 必须覆盖上报数据中的 `uuid`
- **Permission**: `DynamicMonitoringSummary::Write`

权限配置示例：

```json
{
  "scopes": [
    {"agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"}
  ],
  "permissions": [
    {"dynamic_monitoring_summary": "write"}
  ]
}
```

### 返回值

上报成功后返回缓冲状态。数据会先缓存在内存中，由后台定时批量写入数据库：

```json
{
  "status": "buffered"
}
```

### 完整示例

请求：

```json
{
  "jsonrpc": "2.0",
  "method": "agent_report_dynamic_summary",
  "params": {
    "token": "demo_key:demo_secret",
    "dynamic_monitoring_summary_data": {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "time": 1769344168646,
      "cpu_usage": 40,
      "gpu_usage": 5,
      "used_memory": 27062329344,
      "total_memory": 68501925888,
      "available_memory": 41439596544,
      "load_one": 0,
      "load_five": 0,
      "load_fifteen": 0,
      "uptime": 6970,
      "boot_time": 1769337198,
      "process_count": 313,
      "total_space": 322057531392,
      "available_space": 91563786240,
      "read_speed": 35741,
      "write_speed": 49550,
      "tcp_connections": 165,
      "udp_connections": 67,
      "total_received": 527863209,
      "total_transmitted": 484144450,
      "transmit_speed": 1626,
      "receive_speed": 5559
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
    "status": "buffered"
  }
}
```
