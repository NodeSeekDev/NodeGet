use crate::AGENT_CONFIG;
use crate::monitoring::gpu::{DynamicDataFromGpu, StaticDataFromGpu};
use crate::monitoring::network_connections::calc_connections;
use crate::monitoring::system_impls::{DynamicDataFromSystem, StaticDataFromSystem};
use crate::monitoring::{refresh_global_disk, refresh_global_network};
use nodeget_lib::monitoring::data_structure::DiskKind::{Hdd, Ssd, Unknown};
use nodeget_lib::monitoring::data_structure::{
    DynamicMonitoringData, DynamicNetworkData, DynamicPerDiskData, DynamicPerNetworkInterfaceData,
    StaticMonitoringData,
};
use nodeget_lib::utils::get_local_timestamp_ms;
use sysinfo::DiskKind;

// 监控数据获取 trait，定义了刷新和获取监控数据的方法
pub trait Monitor {
    // 异步刷新并获取监控数据
    //
    // # 返回值
    // 返回实现了此 trait 的类型的实例
    async fn refresh_and_get() -> Self;
}

// 静态监控数据的 Monitor trait 实现

impl Monitor for StaticMonitoringData {
    // 异步刷新并获取静态监控数据
    //
    // 该函数并发获取系统和GPU的静态数据，然后构造静态监控数据结构
    //
    // # 返回值
    // 返回包含代理 UUID、时间戳以及 CPU、系统和 GPU 静态数据的静态监控数据结构
    async fn refresh_and_get() -> Self {
        let (system_data, gpu_data) =
            tokio::join!(StaticDataFromSystem::get(), StaticDataFromGpu::get());
        Self {
            uuid: AGENT_CONFIG.get().unwrap().agent_uuid.clone().to_string(),
            time: get_local_timestamp_ms(),

            cpu: system_data.0.clone(),
            system: system_data.1.clone(),
            gpu: gpu_data.0.clone(),
        }
    }
}

// 动态监控数据的 Monitor trait 实现

impl Monitor for DynamicMonitoringData {
    // 异步刷新并获取动态监控数据
    //
    // 该函数获取系统的动态数据（CPU、内存、负载、系统），并并发获取磁盘和网络数据，
    // 最后构造动态监控数据结构
    //
    // # 返回值
    // 返回包含代理 UUID、时间戳以及 CPU、内存、负载、系统、磁盘、网络和 GPU 动态数据的动态监控数据结构
    async fn refresh_and_get() -> Self {
        let system_guard = DynamicDataFromSystem::refresh_and_get().await;
        let (cpu, ram, load, system) = (
            system_guard.0.clone(),
            system_guard.1.clone(),
            system_guard.2.clone(),
            system_guard.3.clone(),
        );
        drop(system_guard);

        let handle_disk = tokio::spawn(DataFromDisk::refresh_and_get());
        let handle_network = tokio::spawn(DataFromNetwork::refresh_and_get());

        let gpu_data = {
            let gpu_guard = DynamicDataFromGpu::refresh_and_get().await;
            gpu_guard.0.clone()
        };

        let disk_data = handle_disk.await.unwrap();
        let network_data = handle_network.await.unwrap();

        Self {
            uuid: AGENT_CONFIG.get().unwrap().agent_uuid.clone().to_string(),
            time: get_local_timestamp_ms(),

            cpu,
            ram,
            load,
            system,
            disk: disk_data.0,
            network: network_data.0,
            gpu: gpu_data,
        }
    }
}

// 磁盘监控相关功能

// 从磁盘获取的数据结构，包含磁盘的动态数据
#[derive(Debug)]
pub struct DataFromDisk(pub Vec<DynamicPerDiskData>);

impl DataFromDisk {
    // 异步刷新并获取磁盘数据
    //
    // 该函数刷新全局磁盘信息，计算磁盘读写速度，并收集每个磁盘的动态数据
    //
    // # 返回值
    // 返回包含所有磁盘动态数据的向量
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_disk().await.as_secs_f64();
        let disk_mutex = crate::monitoring::get_global_disk().await;
        let per_disk_vec = {
            let disks = disk_mutex.lock().await;
            disks
                .iter()
                .map(|disk| {
                    let usage = disk.usage();

                    DynamicPerDiskData {
                        kind: match disk.kind() {
                            DiskKind::HDD => Hdd,
                            DiskKind::SSD => Ssd,
                            DiskKind::Unknown(_) => Unknown,
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
                .collect::<Vec<_>>()
        };

        Self(per_disk_vec)
    }
}

// 网络监控相关功能

// 从网络获取的数据结构，包含网络的动态数据
#[derive(Debug)]
pub struct DataFromNetwork(pub DynamicNetworkData);

impl DataFromNetwork {
    // 异步刷新并获取网络数据
    //
    // 该函数刷新全局网络信息，计算网络接口的传输速度，并统计 UDP 和 TCP 连接数
    //
    // # 返回值
    // 返回包含网络接口数据以及 UDP 和 TCP 连接数的网络数据结构
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_network().await.as_secs_f64();
        let networks_mutex = crate::monitoring::get_global_network().await;
        let network_vec = {
            let networks = networks_mutex.lock().await;
            networks
                .iter()
                .map(|(interface_name, network)| DynamicPerNetworkInterfaceData {
                    interface_name: interface_name.clone(),
                    total_received: network.total_received(),
                    total_transmitted: network.total_transmitted(),
                    receive_speed: (network.received() as f64 / interval_secs) as u64,
                    transmit_speed: (network.transmitted() as f64 / interval_secs) as u64,
                })
                .collect()
        };

        let (udp_connections, tcp_connections) = calc_connections();

        Self(DynamicNetworkData {
            interfaces: network_vec,
            udp_connections,
            tcp_connections,
        })
    }
}
