// 若数据量字段中未注明单位，则以字节 (Bytes) 为单位
// 若速度字段中未注明单位，则以字节每秒 (Bytes per second) 为单位

#[derive(Debug)]
pub struct MonitoringData {
    pub cpu: CPUData,
    pub ram: RamData,
    pub load: LoadData,
    pub system: SystemData,
    pub disk: Vec<PerDiskData>,
    pub network: NetworkData,
}

#[derive(Debug, Clone)]
pub struct CPUData {
    // 不变
    pub physical_cores: u64,
    pub logical_cores: u64,

    // 变
    pub per_core: Vec<PerCpuCoreData>,
    pub total_cpu_usage: f64,
}

#[derive(Debug, Clone)]
pub struct PerCpuCoreData {
    // 不变
    pub name: String,
    pub vendor_id: String,
    pub brand: String,

    // 变
    pub cpu_usage: f64,
    pub frequency_mhz: u64,
}

#[derive(Debug, Clone)]
pub struct RamData {
    // 变
    pub total_memory: u64,
    pub available_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
}

#[derive(Debug, Clone)]
pub struct LoadData {
    // 变
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone)]
pub struct SystemData {
    // 不变
    pub system_name: String,
    pub system_kernel: String,
    pub system_kernel_version: String,
    pub system_os_version: String,
    pub system_os_long_version: String,
    pub distribution_id: String,
    pub system_host_name: String,
    pub arch: String,
    pub virtualization: String,

    // 变
    pub boot_time: u64,
    pub uptime: u64,
    pub process_count: u64,
}

#[derive(Debug, Clone)]
pub struct PerDiskData {
    // 变
    pub kind: String, // e.g., SSD, HDD
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

#[derive(Debug)]
pub struct NetworkData {
    pub interfaces: Vec<PerNetworkInterfaceData>,
    pub udp_connections: u64,
    pub tcp_connections: u64,
}

#[derive(Debug, Clone)]
pub struct PerNetworkInterfaceData {
    // 变
    pub interface_name: String,
    pub total_received: u64,    // 从上次网卡重启开始计算
    pub total_transmitted: u64, // 从上次网卡重启开始计算
    pub receive_speed: u64,
    pub transmit_speed: u64,
}
