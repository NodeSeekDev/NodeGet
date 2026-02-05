// 若数据量字段中未注明单位，则以字节 (Bytes) 为单位

// 静态监控数据结构体，包含不会随时间变化的硬件信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticMonitoringData {
    // 设备 UUID
    pub uuid: String,
    // 时间戳（毫秒）
    pub time: u64,

    // CPU 静态信息
    pub cpu: StaticCPUData,
    // 系统静态信
    pub system: StaticSystemData,
    // GPU 静态信息列表
    pub gpu: Vec<StaticGpuData>,
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
