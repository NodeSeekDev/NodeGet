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

pub trait Monitor {
    async fn refresh_and_get() -> Self;
}

// Monitoring (ALL)

impl Monitor for StaticMonitoringData {
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

impl Monitor for DynamicMonitoringData {
    async fn refresh_and_get() -> Self {
        let (cpu, ram, load, system) = {
            let guard_tuple = tokio::join!(DynamicDataFromSystem::refresh_and_get()).0;
            (
                guard_tuple.0.clone(),
                guard_tuple.1.clone(),
                guard_tuple.2.clone(),
                guard_tuple.3.clone(),
            )
        };

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

// Disk

#[derive(Debug)]
pub struct DataFromDisk(pub Vec<DynamicPerDiskData>);

impl DataFromDisk {
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_disk().await.as_secs_f64();
        let disk_mutex = crate::monitoring::get_global_disk().await;
        let disks = disk_mutex.lock().await;

        let per_disk_vec = disks
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
            .collect::<Vec<_>>();

        Self(per_disk_vec)
    }
}

// Network

#[derive(Debug)]
pub struct DataFromNetwork(pub DynamicNetworkData);

impl DataFromNetwork {
    pub async fn refresh_and_get() -> Self {
        let interval_secs = refresh_global_network().await.as_secs_f64();
        let networks_mutex = crate::monitoring::get_global_network().await;
        let networks = networks_mutex.lock().await;

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

        Self(DynamicNetworkData {
            interfaces: network_vec,
            udp_connections,
            tcp_connections,
        })
    }
}
