# Monitoring 总览

Monitoring 是本项目的核心功能之一，负责系统监控数据的上报与查询。Agent 定期采集主机的静态/动态信息，通过 JSON-RPC 上报至
Server，调用者可按条件查询历史数据。

## 数据类型

监控数据分为三大类：

- **StaticMonitoringData**: 静态信息，采集后一般不会变化（CPU 型号、系统版本、GPU 型号等）
- **DynamicMonitoringData**: 动态信息，随系统实时变化（CPU 使用率、内存、磁盘、网络等）
- **DynamicMonitoringSummaryData**: 动态摘要信息，将动态数据的关键指标扁平化存储（非 JSON），便于高效查询与聚合

## 上报结构体

### StaticMonitoringData

```rust
pub struct StaticMonitoringData {
    pub uuid: String,               // Agent UUID
    pub time: u64,                  // 毫秒时间戳
    pub data_hash: Vec<u8>,         // 数据内容 SHA-256 哈希（前 16 字节原始二进制），用于去重
    pub cpu: StaticCPUData,
    pub system: StaticSystemData,
    pub gpu: Vec<StaticGpuData>,
}
```

JSON 示例：

```json
{
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
      },
      {
        "id": 2,
        "name": "CPU 2",
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
```

子结构体定义：

```rust
pub struct StaticCPUData {
    pub physical_cores: u64,
    pub logical_cores: u64,
    pub per_core: Vec<StaticPerCpuCoreData>,
}

pub struct StaticPerCpuCoreData {
    pub id: u32,            // ID 从 1 开始
    pub name: String,
    pub vendor_id: String,
    pub brand: String,
}

pub struct StaticSystemData {
    pub system_name: String,
    pub system_kernel: String,
    pub system_kernel_version: String,
    pub system_os_version: String,
    pub system_os_long_version: String,
    pub distribution_id: String,
    pub system_host_name: String,
    pub arch: String,
    pub virtualization: String,
}

pub struct StaticGpuData {
    pub id: u32,            // ID 从 1 开始
    pub name: String,
    pub cuda_cores: u64,    // NVIDIA 独有
    pub architecture: String,
}
```

### DynamicMonitoringData

```rust
pub struct DynamicMonitoringData {
    pub uuid: String,                       // Agent UUID
    pub time: u64,                          // 毫秒时间戳
    pub cpu: DynamicCPUData,
    pub ram: DynamicRamData,
    pub load: DynamicLoadData,
    pub system: DynamicSystemData,
    pub disk: Vec<DynamicPerDiskData>,
    pub network: DynamicNetworkData,
    pub gpu: Vec<DynamicGpuData>,
}
```

JSON 示例：

```json
{
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
  "time": 1769344168646,
  "cpu": {
    "per_core": [
      {
        "id": 1,
        "cpu_usage": 13.43,
        "frequency_mhz": 2007
      },
      {
        "id": 2,
        "cpu_usage": 1.81,
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
```

子结构体定义：

```rust
pub struct DynamicCPUData {
    pub per_core: Vec<DynamicPerCpuCoreData>,
    pub total_cpu_usage: f64,           // 0~100
}

pub struct DynamicPerCpuCoreData {
    pub id: u32,                        // ID 从 1 开始
    pub cpu_usage: f64,                 // 0~100
    pub frequency_mhz: u64,
}

pub struct DynamicRamData {
    pub total_memory: u64,              // 单位字节
    pub available_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
}

pub struct DynamicLoadData {
    pub one: f64,                       // 仅 Linux / macOS 有效
    pub five: f64,
    pub fifteen: f64,
}

pub struct DynamicSystemData {
    pub boot_time: u64,                 // 秒时间戳
    pub uptime: u64,                    // 秒
    pub process_count: u64,
}

pub enum DiskKind { Hdd, Ssd, Unknown }

pub struct DynamicPerDiskData {
    pub kind: DiskKind,
    pub name: String,
    pub file_system: String,
    pub mount_point: String,
    pub total_space: u64,               // 单位字节
    pub available_space: u64,
    pub is_removable: bool,
    pub is_read_only: bool,
    pub read_speed: u64,                // 字节/秒
    pub write_speed: u64,
}

pub struct DynamicNetworkData {
    pub interfaces: Vec<DynamicPerNetworkInterfaceData>,
    pub udp_connections: u64,
    pub tcp_connections: u64,
}

pub struct DynamicPerNetworkInterfaceData {
    pub interface_name: String,
    pub total_received: u64,            // 单位字节
    pub total_transmitted: u64,
    pub receive_speed: u64,             // 字节/秒
    pub transmit_speed: u64,
}

pub struct DynamicGpuData {
    pub id: u32,                        // ID 从 1 开始
    pub used_memory: u64,               // 单位字节
    pub total_memory: u64,
    pub graphics_clock_mhz: u64,
    pub sm_clock_mhz: u64,
    pub memory_clock_mhz: u64,
    pub video_clock_mhz: u64,
    pub utilization_gpu: u8,            // GPU 使用率
    pub utilization_memory: u8,         // 显存读写频率（非使用率）
    pub temperature: u8,                // 温度
}
```

### DynamicMonitoringSummaryData

动态摘要数据将 DynamicMonitoringData 中的关键指标扁平化为独立列存储，所有字段（除 `uuid` 和 `time`）均为可选。

```rust
pub struct DynamicMonitoringSummaryData {
    pub uuid: String,                       // Agent UUID
    pub time: u64,                          // 毫秒时间戳
    pub cpu_usage: Option<i16>,             // CPU 总使用率 × 10 (0~1000)，查询时自动 /10 还原
    pub gpu_usage: Option<i16>,             // GPU 使用率 (0~100)，整数
    pub used_swap: Option<i64>,             // 已用 Swap (字节)
    pub total_swap: Option<i64>,            // 总 Swap (字节)
    pub used_memory: Option<i64>,           // 已用内存 (字节)
    pub total_memory: Option<i64>,          // 总内存 (字节)
    pub available_memory: Option<i64>,      // 可用内存 (字节)
    pub load_one: Option<i16>,              // 1 分钟负载 × 10，查询时自动 /10 还原
    pub load_five: Option<i16>,             // 5 分钟负载 × 10，查询时自动 /10 还原
    pub load_fifteen: Option<i16>,          // 15 分钟负载 × 10，查询时自动 /10 还原
    pub uptime: Option<i32>,                // 运行时间 (秒)
    pub boot_time: Option<i64>,             // 启动时间 (秒时间戳)
    pub process_count: Option<i32>,         // 进程数
    pub total_space: Option<i64>,           // 磁盘总空间 (字节)
    pub available_space: Option<i64>,       // 磁盘可用空间 (字节)
    pub read_speed: Option<i64>,            // 磁盘读速度 (字节/秒)
    pub write_speed: Option<i64>,           // 磁盘写速度 (字节/秒)
    pub tcp_connections: Option<i32>,       // TCP 连接数
    pub udp_connections: Option<i32>,       // UDP 连接数
    pub total_received: Option<i64>,        // 网络总接收 (字节)
    pub total_transmitted: Option<i64>,     // 网络总发送 (字节)
    pub transmit_speed: Option<i64>,        // 网络发送速度 (字节/秒)
    pub receive_speed: Option<i64>,         // 网络接收速度 (字节/秒)
}
```

JSON 示例：

```json
{
  "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
  "time": 1769344168646,
  "cpu_usage": 40,
  "gpu_usage": 5,
  "used_swap": 0,
  "total_swap": 0,
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
```

与 DynamicMonitoringData 的区别：

- 所有字段均为扁平的基本类型（非嵌套 JSON），便于数据库直接索引和聚合
- 除 `uuid` 和 `time` 外，所有字段均为可选，可按需上报
- `cpu_usage`、`load_one`、`load_five`、`load_fifteen` 在存储时乘以 10 转为整数（`i16`），查询时自动除以 10 还原为浮点数，对调用者透明
- 适合用于仪表盘展示、趋势分析等场景

### 注意事项

- 所有字段都是必需的，若没有请留空（而不是不定义或传 `null`）
- 目前不会对字符串字段做内容检测，请保证上传数据可以被公众展示，勿携带隐私信息
- 多 CPU 核心、多 GPU 时，请确保 Static 与 Dynamic 数据中 `id` 一一对应
- 由于各系统获取到的信息不尽相同，请尽力保证与官方 `nodeget-agent` 实现相同

## 查询条件

### DataQueryField

查询时通过 `fields` 指定需要返回的数据字段：

- **StaticDataQueryField**: `cpu` / `system` / `gpu`
- **DynamicDataQueryField**: `cpu` / `ram` / `load` / `system` / `disk` / `network` / `gpu`
- **DynamicSummaryQueryField**: `cpu_usage` / `gpu_usage` / `used_swap` / `total_swap` / `used_memory` / `total_memory` /
  `available_memory` / `load_one` / `load_five` / `load_fifteen` / `uptime` / `boot_time` / `process_count` / `total_space` /
  `available_space` / `read_speed` / `write_speed` / `tcp_connections` / `udp_connections` / `total_received` /
  `total_transmitted` / `transmit_speed` / `receive_speed`

```rust
#[serde(rename_all = "snake_case")]
pub enum StaticDataQueryField { Cpu, System, Gpu }

#[serde(rename_all = "snake_case")]
pub enum DynamicDataQueryField { Cpu, Ram, Load, System, Disk, Network, Gpu }

#[serde(rename_all = "snake_case")]
pub enum DynamicSummaryQueryField {
    CpuUsage, GpuUsage, UsedSwap, TotalSwap, UsedMemory, TotalMemory,
    AvailableMemory, LoadOne, LoadFive, LoadFifteen, Uptime, BootTime,
    ProcessCount, TotalSpace, AvailableSpace, ReadSpeed, WriteSpeed,
    TcpConnections, UdpConnections, TotalReceived, TotalTransmitted,
    TransmitSpeed, ReceiveSpeed,
}
```

当 `fields` 为空时，要求 Token 至少对一种字段有 `Read` 权限即可；当 `fields` 非空时，Token 必须对指定的每个字段都有 `Read`
权限。

### QueryCondition

查询和删除都使用统一的条件结构体：

```rust
#[serde(rename_all = "snake_case")]
pub enum QueryCondition {
    Uuid(uuid::Uuid),
    TimestampFromTo(i64, i64),  // start, end
    TimestampFrom(i64),
    TimestampTo(i64),
    Limit(u64),
    Last,
}
```

JSON 解析示例：

```json
{ "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd" }
```

```json
{ "timestamp_from_to": [1769344168646, 1769344169646] }
```

```json
{ "timestamp_from": 1769344168646 }
```

```json
{ "limit": 1000 }
```

```json
"last"
```

注意事项：

- `timestamp_from_to` 等价于同时传 `timestamp_from` 和 `timestamp_to`
- `limit` 为 1 与 `last` 等价，按时间倒序取最新记录
- 多个条件并存时为 `AND` 关系，只查询满足所有条件的数据

### AvgQuery 结构体

用于 `agent_query_static_avg` / `agent_query_dynamic_avg` / `agent_query_dynamic_summary_avg` 的参数结构：

```rust
pub struct StaticDataAvgQuery {
    pub fields: Vec<StaticDataQueryField>,
    pub uuid: uuid::Uuid,
    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,
    pub points: u64,
}

pub struct DynamicDataAvgQuery {
    pub fields: Vec<DynamicDataQueryField>,
    pub uuid: uuid::Uuid,
    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,
    pub points: u64,
}

pub struct DynamicSummaryAvgQuery {
    pub fields: Vec<DynamicSummaryQueryField>,
    pub uuid: uuid::Uuid,
    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,
    pub points: u64,
}
```

JSON 示例：

```json
{
  "fields": ["cpu", "ram", "load", "system"],
  "uuid": "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3",
  "timestamp_from": 1769344168646,
  "timestamp_to": 1769347768646,
  "points": 100
}
```

- `timestamp_from` / `timestamp_to` 可省略
- `points` 必须 >= 1
- 仅用于 PostgreSQL，不支持 SQLite

## 查询返回结构

查询返回的数据结构与上报结构体略有不同：

- `uuid` 和 `timestamp` 字段固定包含
- 其余字段根据 `fields` 参数按需返回（未请求的字段不包含在结果中）

```rust
pub struct StaticResponseItem {
    pub uuid: String,
    pub timestamp: i64,
    pub cpu: Option<Value>,
    pub system: Option<Value>,
    pub gpu: Option<Value>,
}

pub struct DynamicResponseItem {
    pub uuid: String,
    pub timestamp: i64,
    pub cpu: Option<Value>,
    pub ram: Option<Value>,
    pub load: Option<Value>,
    pub system: Option<Value>,
    pub disk: Option<Value>,
    pub network: Option<Value>,
    pub gpu: Option<Value>,
}

pub struct DynamicSummaryResponseItem {
    pub uuid: String,
    pub timestamp: i64,
    pub cpu_usage: Option<Value>,
    pub gpu_usage: Option<Value>,
    pub used_swap: Option<Value>,
    pub total_swap: Option<Value>,
    pub used_memory: Option<Value>,
    pub total_memory: Option<Value>,
    pub available_memory: Option<Value>,
    pub load_one: Option<Value>,
    pub load_five: Option<Value>,
    pub load_fifteen: Option<Value>,
    pub uptime: Option<Value>,
    pub boot_time: Option<Value>,
    pub process_count: Option<Value>,
    pub total_space: Option<Value>,
    pub available_space: Option<Value>,
    pub read_speed: Option<Value>,
    pub write_speed: Option<Value>,
    pub tcp_connections: Option<Value>,
    pub udp_connections: Option<Value>,
    pub total_received: Option<Value>,
    pub total_transmitted: Option<Value>,
    pub transmit_speed: Option<Value>,
    pub receive_speed: Option<Value>,
}
```

## 相关页面

- [Agent 上报](./agent.md) — `agent_report_static` / `agent_report_dynamic` / `agent_report_dynamic_summary`
- [查询与删除](./query.md) — 查询、分段平均、批量最新、删除
