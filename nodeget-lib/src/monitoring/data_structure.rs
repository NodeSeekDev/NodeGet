// 若数据量字段中未注明单位，则以字节 (Bytes) 为单位

use sha2::{Digest, Sha256};

// 静态监控数据结构体，包含不会随时间变化的硬件信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticMonitoringData {
    // 设备 UUID
    pub uuid: String,
    // 时间戳（毫秒）
    pub time: u64,
    // 数据内容的 SHA-256 哈希（前 16 字节原始二进制），用于去重
    pub data_hash: Vec<u8>,

    // CPU 静态信息
    pub cpu: StaticCPUData,
    // 系统静态信
    pub system: StaticSystemData,
    // GPU 静态信息列表
    pub gpu: Vec<StaticGpuData>,
}

impl StaticMonitoringData {
    /// 根据 cpu / system / gpu 三个字段的内容计算确定性 SHA-256 哈希。
    ///
    /// 内部将三个字段各自序列化为 `serde_json::Value`，再递归排序所有 object key，
    /// 拼接为一个确定性字符串后取 SHA-256。
    /// 同一组数据无论 JSON 序列化时 key 顺序如何，都会得到相同的哈希值。
    pub fn compute_data_hash(
        cpu: &StaticCPUData,
        system: &StaticSystemData,
        gpu: &[StaticGpuData],
    ) -> Vec<u8> {
        fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
            match v {
                serde_json::Value::Object(map) => {
                    let mut sorted: Vec<(&String, serde_json::Value)> =
                        map.iter().map(|(k, v)| (k, canonicalize(v))).collect();
                    sorted.sort_by(|a, b| a.0.cmp(b.0));
                    serde_json::Value::Object(
                        sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect(),
                    )
                }
                serde_json::Value::Array(arr) => {
                    serde_json::Value::Array(arr.iter().map(canonicalize).collect())
                }
                other => other.clone(),
            }
        }

        let cpu_val = canonicalize(&serde_json::to_value(cpu).unwrap());
        let sys_val = canonicalize(&serde_json::to_value(system).unwrap());
        let gpu_val = canonicalize(&serde_json::to_value(gpu).unwrap());

        let canonical = format!("{}\n{}\n{}", cpu_val, sys_val, gpu_val);

        let hash = Sha256::digest(canonical.as_bytes());
        // 取前 16 字节 (128 bit) 足够去重
        hash[..16].to_vec()
    }
}

// 动态监控数据结构体，包含随时间变化的系统状态信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringData {
    // 设备 UUID
    pub uuid: String,
    // 时间戳（毫秒）
    pub time: u64,

    // CPU 动态信息
    pub cpu: DynamicCPUData,
    // 内存动态信息
    pub ram: DynamicRamData,
    // 系统负载动态信息
    pub load: DynamicLoadData,
    // 系统动态信息
    pub system: DynamicSystemData,
    // 磁盘动态信息列表
    pub disk: Vec<DynamicPerDiskData>,
    // 网络动态信息
    pub network: DynamicNetworkData,
    // GPU 动态信息列表
    pub gpu: Vec<DynamicGpuData>,
}

// 动态监控摘要数据结构体，包含扁平化的系统状态摘要信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringSummaryData {
    // 设备 UUID
    pub uuid: String,
    // 时间戳（毫秒）
    pub time: u64,

    pub cpu_usage: Option<i16>,
    pub gpu_usage: Option<i16>,
    pub used_swap: Option<i64>,
    pub total_swap: Option<i64>,
    pub used_memory: Option<i64>,
    pub total_memory: Option<i64>,
    pub available_memory: Option<i64>,
    pub load_one: Option<i16>,
    pub load_five: Option<i16>,
    pub load_fifteen: Option<i16>,
    pub uptime: Option<i32>,
    pub boot_time: Option<i64>,
    pub process_count: Option<i32>,
    pub total_space: Option<i64>,
    pub available_space: Option<i64>,
    pub read_speed: Option<i64>,
    pub write_speed: Option<i64>,
    pub tcp_connections: Option<i32>,
    pub udp_connections: Option<i32>,
    pub total_received: Option<i64>,
    pub total_transmitted: Option<i64>,
    pub transmit_speed: Option<i64>,
    pub receive_speed: Option<i64>,
}

impl From<&DynamicMonitoringData> for DynamicMonitoringSummaryData {
    fn from(data: &DynamicMonitoringData) -> Self {
        let total_space: u64 = data.disk.iter().map(|d| d.total_space).sum();
        let available_space: u64 = data.disk.iter().map(|d| d.available_space).sum();
        let read_speed: u64 = data.disk.iter().map(|d| d.read_speed).sum();
        let write_speed: u64 = data.disk.iter().map(|d| d.write_speed).sum();

        let total_received: u64 = data
            .network
            .interfaces
            .iter()
            .map(|i| i.total_received)
            .sum();
        let total_transmitted: u64 = data
            .network
            .interfaces
            .iter()
            .map(|i| i.total_transmitted)
            .sum();
        let receive_speed_net: u64 = data
            .network
            .interfaces
            .iter()
            .map(|i| i.receive_speed)
            .sum();
        let transmit_speed: u64 = data
            .network
            .interfaces
            .iter()
            .map(|i| i.transmit_speed)
            .sum();

        Self {
            uuid: data.uuid.clone(),
            time: data.time,
            cpu_usage: Some(
                (data.cpu.total_cpu_usage * 10.0).clamp(i16::MIN as f64, i16::MAX as f64) as i16,
            ),
            gpu_usage: data.gpu.first().map(|g| g.utilization_gpu as i16),
            used_swap: Some(data.ram.used_swap as i64),
            total_swap: Some(data.ram.total_swap as i64),
            used_memory: Some(data.ram.used_memory as i64),
            total_memory: Some(data.ram.total_memory as i64),
            available_memory: Some(data.ram.available_memory as i64),
            load_one: Some((data.load.one * 10.0).clamp(i16::MIN as f64, i16::MAX as f64) as i16),
            load_five: Some((data.load.five * 10.0).clamp(i16::MIN as f64, i16::MAX as f64) as i16),
            load_fifteen: Some(
                (data.load.fifteen * 10.0).clamp(i16::MIN as f64, i16::MAX as f64) as i16,
            ),
            uptime: Some(data.system.uptime as i32),
            boot_time: Some(data.system.boot_time as i64),
            process_count: Some(data.system.process_count as i32),
            total_space: Some(total_space as i64),
            available_space: Some(available_space as i64),
            read_speed: Some(read_speed as i64),
            write_speed: Some(write_speed as i64),
            tcp_connections: Some(data.network.tcp_connections as i32),
            udp_connections: Some(data.network.udp_connections as i32),
            total_received: Some(total_received as i64),
            total_transmitted: Some(total_transmitted as i64),
            transmit_speed: Some(transmit_speed as i64),
            receive_speed: Some(receive_speed_net as i64),
        }
    }
}

// CPU 静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticCPUData {
    // 物理核心数
    pub physical_cores: u64,
    // 逻辑核心数
    pub logical_cores: u64,
    // 每个 CPU 核心的静态信息列表
    pub per_core: Vec<StaticPerCpuCoreData>,
}

// CPU 动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicCPUData {
    // 每个 CPU 核心的动态信息列表
    pub per_core: Vec<DynamicPerCpuCoreData>,
    // CPU 总使用率（0-100）
    pub total_cpu_usage: f64,
}

// 每个 CPU 核心的静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticPerCpuCoreData {
    // 核心 ID，从 1 开始
    pub id: u32,
    // 核心名称
    pub name: String,
    // 供应商 ID
    pub vendor_id: String,
    // CPU 品牌
    pub brand: String,
}

// 每个 CPU 核心的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerCpuCoreData {
    // 核心 ID，从 1 开始
    pub id: u32,
    // CPU 使用率（0-100）
    pub cpu_usage: f64,
    // CPU 频率（MHz）
    pub frequency_mhz: u64,
}

// 内存动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicRamData {
    // 总内存大小（字节）
    pub total_memory: u64,
    // 可用内存大小（字节）
    pub available_memory: u64,
    // 已使用内存大小（字节）
    pub used_memory: u64,
    // 总交换空间大小（字节）
    pub total_swap: u64,
    // 已使用交换空间大小（字节）
    pub used_swap: u64,
}

// 系统负载动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicLoadData {
    // 1分钟平均负载
    pub one: f64,
    // 5分钟平均负载
    pub five: f64,
    // 15分钟平均负载
    pub fifteen: f64,
}

// 系统静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticSystemData {
    // 系统名称
    pub system_name: String,
    // 系统内核版本
    pub system_kernel: String,
    // 系统内核详细版本
    pub system_kernel_version: String,
    // 系统操作系统版本
    pub system_os_version: String,
    // 系统操作系统详细版本
    pub system_os_long_version: String,
    // 发行版 ID
    pub distribution_id: String,
    // 系统主机名
    pub system_host_name: String,
    // 系统架构
    pub arch: String,
    // 虚拟化平台
    pub virtualization: String,
}

// 系统动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicSystemData {
    // 系统启动时间（秒时间戳）
    pub boot_time: u64,
    // 系统运行时间（秒）
    pub uptime: u64,
    // 进程数量
    pub process_count: u64,
}

// 磁盘类型枚举
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum DiskKind {
    // 机械硬盘
    Hdd,
    // 固态硬盘
    Ssd,
    // 未知类型
    Unknown,
}

// 每个磁盘的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerDiskData {
    // 磁盘类型
    pub kind: DiskKind,
    // 磁盘名称
    pub name: String,
    // 文件系统类型
    pub file_system: String,
    // 挂载点
    pub mount_point: String,
    // 总空间大小（字节）
    pub total_space: u64,
    // 可用空间大小（字节）
    pub available_space: u64,
    // 是否可移动
    pub is_removable: bool,
    // 是否只读
    pub is_read_only: bool,
    // 读取速度（字节/秒）
    pub read_speed: u64,
    // 写入速度（字节/秒）
    pub write_speed: u64,
}

// 网络动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicNetworkData {
    // 网络接口列表
    pub interfaces: Vec<DynamicPerNetworkInterfaceData>,
    // UDP 连接数
    pub udp_connections: u64,
    // TCP 连接数
    pub tcp_connections: u64,
}

// 每个网络接口的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerNetworkInterfaceData {
    // 网络接口名称
    pub interface_name: String,
    // 总接收数据量（字节），从上次网卡重启开始计算
    pub total_received: u64, // 从上次网卡重启开始计算
    // 总发送数据量（字节），从上次网卡重启开始计算
    pub total_transmitted: u64, // 从上次网卡重启开始计算
    // 接收速度（字节/秒）
    pub receive_speed: u64,
    // 发送速度（字节/秒）
    pub transmit_speed: u64,
}

// GPU 静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticGpuData {
    // GPU ID，从 1 开始
    pub id: u32,
    // GPU 名称
    pub name: String,
    // CUDA 核心数（对于非 NVIDIA 显卡，该值为 0）
    pub cuda_cores: u64, // 对于非 NVIDIA 显卡，该值为 0
    // GPU 架构
    pub architecture: String,
}

// GPU 动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicGpuData {
    // GPU ID，从 1 开始
    pub id: u32,
    // 已使用显存（字节）
    pub used_memory: u64,
    // 总显存（字节）
    pub total_memory: u64,
    // 图形时钟频率（MHz）
    pub graphics_clock_mhz: u64,
    // 流处理器时钟频率（MHz），NV: Streaming Multiprocessor; AMD: Compute Unit
    pub sm_clock_mhz: u64, // NV: Streaming Multiprocessor; AMD: Compute Unit
    // 显存时钟频率（MHz）
    pub memory_clock_mhz: u64,
    // 视频时钟频率（MHz）
    pub video_clock_mhz: u64,
    // GPU 使用率百分比
    pub utilization_gpu: u8,
    // 显存使用率百分比 (不是显存占用率，反应内存读写频率的数值)
    pub utilization_memory: u8,
    // 温度（摄氏度）
    pub temperature: u8,
}
