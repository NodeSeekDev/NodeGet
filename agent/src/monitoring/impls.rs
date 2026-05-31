use crate::monitoring::gpu::{DynamicDataFromGpu, StaticDataFromGpu};
use crate::monitoring::network_connections::calc_connections;
use crate::monitoring::system_impls::{DynamicDataFromSystem, StaticDataFromSystem};
use crate::monitoring::{refresh_global_disk, refresh_global_network};
use ng_core::utils::get_local_timestamp_ms;
use ng_monitoring::data_structure::DiskKind::{Hdd, Ssd, Unknown};
use ng_monitoring::data_structure::{
    DynamicMonitoringData, DynamicNetworkData, DynamicPerDiskData, DynamicPerNetworkInterfaceData,
    StaticMonitoringData,
};
use sysinfo::DiskKind;

// 监控数据获取 trait，定义了刷新和获取监控数据的方法
pub trait Monitor {
    // 异步刷新并获取监控数据
    //
    // # 返回值
    // 返回实现了此 trait 的类型的实例
    async fn refresh_and_get() -> Self;
}

// review_agent.md 低优：`get_local_timestamp_ms().unwrap_or(0)` 把失败时间戳化为 1970，会被
// server 误认为是一条真实的"很旧"数据。不改协议类型（`time: u64`）的前提下，至少把失败路径
// 的日志升级为 error，方便事后排查时能在日志里找到这类"上报带 0 时间戳"的事件。
fn timestamp_ms_with_error_log() -> u64 {
    match get_local_timestamp_ms() {
        Ok(ts) => ts,
        Err(e) => {
            log::error!(
                "get_local_timestamp_ms failed, falling back to 0 which server may interpret as 1970: {e}"
            );
            0
        }
    }
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
        let agent_uuid = crate::config_access::current_agent_uuid();

        let cpu = system_data.0.clone();
        let system = system_data.1.clone();
        let gpu = gpu_data.0.clone();
        let data_hash = Self::compute_data_hash(&cpu, &system, &gpu)
            .expect("Static monitoring data should always be serializable");

        Self {
            uuid: agent_uuid,
            time: timestamp_ms_with_error_log(),
            data_hash,
            cpu,
            system,
            gpu,
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
        // 统一 `tokio::join!` 四路并发（对齐静态实现风格），4 个来源互不共享锁/全局资源：
        //   - system: DynamicDataFromSystem 自己的 mutex
        //   - gpu:    NVML mutex（gpu.rs 内部用 block_in_place 避免跨 await 阻塞调度）
        //   - disk:   DISK_TIME_TRACKER + 全局 disks mutex
        //   - network: NETWORK_TIME_TRACKER + 全局 networks mutex
        // 之前版本 (review_agent.md #79) 用 `tokio::spawn + .await`，一是风格不统一，
        // 二是会把每次采集拆成 3 个 task（含 JoinError fallback），这里的四路 await
        // 都在同一个父 task 内——panic 直接 propagate，无须多层 fallback。
        let system_fut = async {
            let system_guard = DynamicDataFromSystem::refresh_and_get().await;
            let cpu = system_guard.0.clone();
            let ram = system_guard.1.clone();
            let load = system_guard.2.clone();
            let system = system_guard.3.clone();
            drop(system_guard);
            (cpu, ram, load, system)
        };
        let gpu_fut = async {
            let gpu_guard = DynamicDataFromGpu::refresh_and_get().await;
            gpu_guard.0.clone()
        };
        let disk_fut = DataFromDisk::refresh_and_get();
        let network_fut = DataFromNetwork::refresh_and_get();

        let ((cpu, ram, load, system), gpu_data, disk_data, network_data) =
            tokio::join!(system_fut, gpu_fut, disk_fut, network_fut);

        let agent_uuid = crate::config_access::current_agent_uuid();

        Self {
            uuid: agent_uuid,
            time: timestamp_ms_with_error_log(),

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
        // 首次 tick 或系统时钟异常可能返回 interval ≈ 0，除法会得到 inf（cast 成
        // u64 后变成 u64::MAX），因此设一个下限。10ms 对应 100Hz 采样粒度，足够安全。
        let safe_interval_secs = interval_secs.max(0.01);
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

                        read_speed: (usage.read_bytes as f64 / safe_interval_secs) as u64,
                        write_speed: (usage.written_bytes as f64 / safe_interval_secs) as u64,
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
        // 同磁盘：首次或时钟异常的近 0 值会使速率变成 u64::MAX，加一个 10ms 下限。
        let safe_interval_secs = interval_secs.max(0.01);
        let networks_mutex = crate::monitoring::get_global_network().await;
        let network_vec = {
            let networks = networks_mutex.lock().await;
            networks
                .iter()
                .map(|(interface_name, network)| DynamicPerNetworkInterfaceData {
                    interface_name: interface_name.clone(),
                    total_received: network.total_received(),
                    total_transmitted: network.total_transmitted(),
                    receive_speed: (network.received() as f64 / safe_interval_secs) as u64,
                    transmit_speed: (network.transmitted() as f64 / safe_interval_secs) as u64,
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
