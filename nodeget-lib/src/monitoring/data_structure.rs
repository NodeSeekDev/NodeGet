// 若数据量字段中未注明单位，则以字节 (Bytes) 为单位
// 若速度字段中未注明单位，则以字节每秒 (Bytes per second) 为单位


#[cfg(feature = "for-server")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "for-agent")]
use miniserde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticMonitoringDataForDatabase {
    pub id: u64,
    pub uuid: String,
    pub data: StaticMonitoringData,
    pub time: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicMonitoringDataForDatabase {
    pub id: u64,
    pub uuid: String,
    pub data: DynamicMonitoringData,
    pub time: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticMonitoringData {
    pub cpu: StaticCPUData,
    pub system: StaticSystemData,
    pub gpu: Vec<StaticGpuData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicMonitoringData {
    pub cpu: DynamicCPUData,
    pub ram: DynamicRamData,
    pub load: DynamicLoadData,
    pub system: DynamicSystemData,
    pub disk: Vec<DynamicPerDiskData>,
    pub network: DynamicNetworkData,
    pub gpu: Vec<DynamicGpuData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticCPUData {
    pub physical_cores: u64,
    pub logical_cores: u64,
    pub per_core: Vec<StaticPerCpuCoreData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicCPUData {
    pub per_core: Vec<DynamicPerCpuCoreData>,
    pub total_cpu_usage: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticPerCpuCoreData {
    pub id: u32,
    pub name: String,
    pub vendor_id: String,
    pub brand: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicPerCpuCoreData {
    pub id: u32,
    pub cpu_usage: f64,
    pub frequency_mhz: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicRamData {
    pub total_memory: u64,
    pub available_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicLoadData {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicSystemData {
    pub boot_time: u64,
    pub uptime: u64,
    pub process_count: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum DiskKind {
    Hdd,
    Ssd,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicPerDiskData {
    pub kind: DiskKind,
    pub name: String,
    pub file_system: String,
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
    pub is_removable: bool,
    pub is_read_only: bool,
    pub read_speed: u64,
    pub write_speed: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicNetworkData {
    pub interfaces: Vec<DynamicPerNetworkInterfaceData>,
    pub udp_connections: u64,
    pub tcp_connections: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicPerNetworkInterfaceData {
    pub interface_name: String,
    pub total_received: u64,    // 从上次网卡重启开始计算
    pub total_transmitted: u64, // 从上次网卡重启开始计算
    pub receive_speed: u64,
    pub transmit_speed: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticGpuData {
    pub id: u32,
    pub name: String,
    pub cuda_cores: u64, // 对于非 NVIDIA 显卡，该值为 0
    pub architecture: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicGpuData {
    pub id: u32,
    pub used_memory: u64,
    pub total_memory: u64,
    pub graphics_clock_mhz: u64,
    pub sm_clock_mhz: u64, // NV: Streaming Multiprocessor; AMD: Compute Unit
    pub memory_clock_mhz: u64,
    pub video_clock_mhz: u64,
    pub utilization_gpu: u8, // GPU 占用率百分比
    pub utilization_memory: u8, // 显存使用率百分比 (不是显存占用率，反应内存读写频率的数值)
    pub temperature: u8,       // 摄氏度
}