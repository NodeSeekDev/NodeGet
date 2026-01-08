use crate::monitoring::gpu::{DynamicDataFromGpu, StaticDataFromGpu};
use crate::monitoring::network_connections::calc_connections;
use crate::monitoring::system_impls::{DynamicDataFromSystem, StaticDataFromSystem};
use crate::monitoring::{refresh_global_disk, refresh_global_network};
use nodeget_lib::monitoring::data_structure::DiskKind::{Hdd, Ssd, Unknown};
use nodeget_lib::monitoring::data_structure::{StaticMonitoringData, DynamicMonitoringData, DynamicPerDiskData, DynamicNetworkData, DynamicPerNetworkInterfaceData};
use sysinfo::DiskKind;

pub trait Monitor {
    async fn refresh_and_get() -> Self;
}

// Monitoring (ALL)

impl Monitor for StaticMonitoringData {
    async fn refresh_and_get() -> Self {
        let (system_data, gpu_data) = tokio::join!(StaticDataFromSystem::get(), StaticDataFromGpu::get());
        StaticMonitoringData {
            cpu: system_data.0.clone(),
            system: system_data.1.clone(),
            gpu: gpu_data.0.clone(),
        }
    }
}

impl Monitor for DynamicMonitoringData {
    async fn refresh_and_get() -> Self {
        let system_data = tokio::join!(DynamicDataFromSystem::refresh_and_get()).0;
        let handle_disk = tokio::spawn(DataFromDisk::refresh_and_get());
        let handle_network = tokio::spawn(DataFromNetwork::refresh_and_get());
        let disk_data = handle_disk.await.unwrap();
        let network_data = handle_network.await.unwrap();
        let gpu = DynamicDataFromGpu::refresh_and_get().await;

        DynamicMonitoringData {
            cpu: system_data.0.clone(),
            ram: system_data.1.clone(),
            load: system_data.2.clone(),
            system: system_data.3.clone(),
            disk: disk_data.0,
            network: network_data.0,
            gpu: gpu.0.clone(),
        }
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
                        DiskKind::HDD => Hdd,
                        DiskKind::SSD => Ssd,
                        DiskKind::Unknown(_) => {
                            Unknown
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
