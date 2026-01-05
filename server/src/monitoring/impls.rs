use crate::monitoring::data_structure::{
    DynamicCPUData, DynamicLoadData, DynamicMonitoringData, DynamicNetworkData,
    DynamicPerCpuCoreData, DynamicPerDiskData, DynamicPerNetworkInterfaceData, DynamicRamData,
    DynamicSystemData, StaticCPUData, StaticMonitoringData, StaticPerCpuCoreData, StaticSystemData,
};
use crate::monitoring::network_connections::calc_connections;
use crate::monitoring::process::count_processes;
use crate::monitoring::virtualization_detect::detect_virtualization;
use crate::monitoring::{refresh_global_disk, refresh_global_network, refresh_global_system};
use parking_lot::{Mutex, MutexGuard};
use sysinfo::{DiskKind, System};
use tokio::sync::OnceCell;
// Monitoring (ALL)

impl StaticMonitoringData {
    pub async fn get() -> Self {
        let system_data = tokio::join!(StaticDataFromSystem::get()).0;
        StaticMonitoringData {
            cpu: system_data.0.clone(),
            system: system_data.1.clone(),
        }
    }
}

impl DynamicMonitoringData {
    pub async fn refresh_and_get() -> Self {
        let system_data = tokio::join!(DynamicDataFromSystem::refresh_and_get()).0;
        let handle_disk = tokio::spawn(DataFromDisk::refresh_and_get());
        let handle_network = tokio::spawn(DataFromNetwork::refresh_and_get());
        let disk_data = handle_disk.await.unwrap();
        let network_data = handle_network.await.unwrap();

        DynamicMonitoringData {
            cpu: system_data.0.clone(),
            ram: system_data.1.clone(),
            load: system_data.2.clone(),
            system: system_data.3.clone(),
            disk: disk_data.0,
            network: network_data.0,
        }
    }
}

// System

#[derive(Debug)]
pub struct StaticDataFromSystem(pub StaticCPUData, pub StaticSystemData);

static GLOBAL_STATIC_DATA_FROM_SYSTEM: OnceCell<Mutex<StaticDataFromSystem>> =
    OnceCell::const_new();

impl StaticDataFromSystem {
    pub async fn new() -> StaticDataFromSystem {
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock();

        let per_core = system
            .cpus()
            .iter()
            .enumerate()
            .map(|(i, cpu)| StaticPerCpuCoreData {
                id: (i + 1) as u32,
                name: cpu.name().to_string(),
                vendor_id: cpu.vendor_id().to_string(),
                brand: cpu.brand().to_string().trim().to_string(),
            })
            .collect::<Vec<_>>();

        let logical_cores = per_core.len() as u64;
        StaticDataFromSystem(
            StaticCPUData {
                physical_cores: System::physical_core_count().unwrap_or(0) as u64,
                logical_cores,
                per_core,
            },
            StaticSystemData {
                system_name: System::name().unwrap_or_default(),
                system_kernel: System::kernel_version().unwrap_or_default(),
                system_kernel_version: System::long_os_version().unwrap_or_default(),
                system_os_version: System::os_version().unwrap_or_default(),
                system_os_long_version: System::long_os_version().unwrap_or_default(),
                distribution_id: System::distribution_id(),
                system_host_name: System::host_name().unwrap_or_default(),
                arch: System::cpu_arch(),
                virtualization: detect_virtualization().await,
            },
        )
    }

    pub async fn get() -> MutexGuard<'static, StaticDataFromSystem> {
        let data_mutex = GLOBAL_STATIC_DATA_FROM_SYSTEM
            .get_or_init(|| async { Mutex::new(StaticDataFromSystem::new().await) })
            .await;

        data_mutex.lock()
    }
}

#[derive(Debug)]
pub struct DynamicDataFromSystem(
    pub DynamicCPUData,
    pub DynamicRamData,
    pub DynamicLoadData,
    pub DynamicSystemData,
);
static GLOBAL_DYNAMIC_DATA_FROM_SYSTEM: OnceCell<Mutex<DynamicDataFromSystem>> =
    OnceCell::const_new();

impl DynamicDataFromSystem {
    async fn new() -> Self {
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock();

        let per_core = system
            .cpus()
            .iter()
            .enumerate()
            .map(|(id, cpu)| DynamicPerCpuCoreData {
                id: (id + 1) as u32,
                cpu_usage: f64::from(cpu.cpu_usage()),
                frequency_mhz: cpu.frequency(),
            })
            .collect::<Vec<_>>();

        DynamicDataFromSystem(
            DynamicCPUData {
                per_core,
                total_cpu_usage: f64::from(system.global_cpu_usage()),
            },
            DynamicRamData {
                total_memory: system.total_memory(),
                available_memory: system.available_memory(),
                used_memory: system.used_memory(),
                total_swap: system.total_swap(),
                used_swap: system.used_swap(),
            },
            {
                let load = System::load_average();
                DynamicLoadData {
                    one: load.one,
                    five: load.five,
                    fifteen: load.fifteen,
                }
            },
            DynamicSystemData {
                boot_time: System::boot_time(),
                uptime: System::uptime(),
                process_count: u64::from(count_processes()),
            },
        )
    }

    async fn update(&mut self) {
        // 仅处理变更数据
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock();

        for (data, cpu) in self.0.per_core.iter_mut().zip(system.cpus()) {
            data.cpu_usage = f64::from(cpu.cpu_usage());
            data.frequency_mhz = cpu.frequency();
        }
        self.0.total_cpu_usage = f64::from(system.global_cpu_usage());

        self.1.available_memory = system.available_memory();
        self.1.used_memory = system.used_memory();
        self.1.used_swap = system.used_swap();
        self.1.total_memory = system.total_memory();
        self.1.total_swap = system.total_swap();

        let load = System::load_average();
        self.2.one = load.one;
        self.2.five = load.five;
        self.2.fifteen = load.fifteen;

        self.3.boot_time = System::boot_time();
        self.3.uptime = System::uptime();
        self.3.process_count = u64::from(count_processes());
    }

    pub async fn refresh_and_get() -> MutexGuard<'static, DynamicDataFromSystem> {
        // 外部调用
        let data_mutex = GLOBAL_DYNAMIC_DATA_FROM_SYSTEM
            .get_or_init(|| async { Mutex::new(DynamicDataFromSystem::new().await) })
            .await;

        let mut data = data_mutex.lock();
        data.update().await;

        data
    }
}

// Disk

#[derive(Debug)]
pub struct DataFromDisk(pub Vec<DynamicPerDiskData>);

impl DataFromDisk {
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_disk().await.as_secs_f64();
        let disk_mutex = crate::monitoring::get_global_disk().await;
        let disks = disk_mutex.lock();

        let per_disk_vec = disks
            .iter()
            .map(|disk| {
                let usage = disk.usage();

                DynamicPerDiskData {
                    kind: match disk.kind() {
                        DiskKind::HDD => crate::monitoring::data_structure::DiskKind::Hdd,
                        DiskKind::SSD => crate::monitoring::data_structure::DiskKind::Ssd,
                        DiskKind::Unknown(_) => {
                            crate::monitoring::data_structure::DiskKind::Unknown
                        }
                    },
                    name: disk.name().to_string_lossy().into_owned(),
                    file_system: disk.file_system().to_string_lossy().into_owned(),
                    mount_point: disk.mount_point().to_string_lossy().into_owned(),
                    total_space: disk.total_space(),
                    available_space: disk.available_space(),
                    is_removable: disk.is_removable(),
                    is_read_only: disk.is_read_only(),

                    read_speed: (usage.read_bytes as f64 / interval_secs) as u64,
                    write_speed: (usage.written_bytes as f64 / interval_secs) as u64,
                }
            })
            .collect::<Vec<_>>();

        DataFromDisk(per_disk_vec)
    }
}

// Network

#[derive(Debug)]
pub struct DataFromNetwork(pub DynamicNetworkData);

impl DataFromNetwork {
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_network().await.as_secs_f64();
        let networks_mutex = crate::monitoring::get_global_network().await;
        let networks = networks_mutex.lock();

        let network_vec = networks
            .iter()
            .map(|(interface_name, network)| DynamicPerNetworkInterfaceData {
                interface_name: interface_name.clone(),
                total_received: network.total_received(),
                total_transmitted: network.total_transmitted(),
                receive_speed: (network.received() as f64 / interval_secs) as u64,
                transmit_speed: (network.transmitted() as f64 / interval_secs) as u64,
            })
            .collect();

        let (udp_connections, tcp_connections) = calc_connections();

        DataFromNetwork(DynamicNetworkData {
            interfaces: network_vec,
            udp_connections,
            tcp_connections,
        })
    }
}
