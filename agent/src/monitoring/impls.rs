//! 监控数据组合与采集 trait 实现。
//!
//! 定义 [`Monitor`] trait 作为监控数据采集的统一接口，
//! 并为 [`StaticMonitoringData`] 和 [`DynamicMonitoringData`] 提供 trait 实现。
//! 同时包含磁盘和网络速率采集的辅助结构体 [`DataFromDisk`] / [`DataFromNetwork`]。

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
use std::sync::Arc;
use sysinfo::DiskKind;

/// 监控数据获取 trait，定义了刷新并获取监控数据的异步接口。
pub trait Monitor {
    /// 异步刷新并获取监控数据。
    ///
    /// 返回实现了此 trait 的类型的实例。
    async fn refresh_and_get() -> Self;
}

/// 获取本地时间戳（毫秒），失败时记录 error 日志并回退为 0。
///
/// `get_local_timestamp_ms().unwrap_or(0)` 把失败时间戳化为 1970，
/// 会被 server 误认为真实的"很旧"数据。不改协议类型（`time: u64`）的前提下，
/// 至少把失败路径的日志升级为 error，方便事后排查。
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

/// 静态监控数据的 [`Monitor`] trait 实现。
impl Monitor for StaticMonitoringData {
    /// 异步刷新并获取静态监控数据。
    ///
    /// 1. 并发获取系统和 GPU 的静态数据
    /// 2. 计算 `data_hash`
    /// 3. 构造静态监控数据结构
    ///
    /// 返回包含代理 UUID、时间戳以及 CPU、系统和 GPU 静态数据的结构体。
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

/// 动态监控数据的 [`Monitor`] trait 实现。
impl Monitor for DynamicMonitoringData {
    /// 异步刷新并获取动态监控数据。
    ///
    /// 1. 四路并发获取系统、GPU、磁盘、网络动态数据
    /// 2. 构造动态监控数据结构
    ///
    /// 返回包含代理 UUID、时间戳以及 CPU、内存、负载、系统、磁盘、网络和 GPU 动态数据的结构体。
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

/// 从磁盘获取的数据结构，包含所有磁盘的动态数据。
#[derive(Debug)]
pub struct DataFromDisk(pub Arc<Vec<DynamicPerDiskData>>);

impl DataFromDisk {
    /// 异步刷新并获取磁盘数据。
    ///
    /// 1. 刷新全局磁盘信息并获取刷新间隔
    /// 2. 计算每个磁盘的读写速率（字节/秒）
    /// 3. 收集每个磁盘的动态数据
    ///
    /// 返回包含所有磁盘动态数据的结构体。
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

        Self(Arc::new(per_disk_vec))
    }
}

/// 从网络获取的数据结构，包含网络接口动态数据及连接统计。
#[derive(Debug)]
pub struct DataFromNetwork(pub DynamicNetworkData);

impl DataFromNetwork {
    /// 异步刷新并获取网络数据。
    ///
    /// 1. 刷新全局网络信息并获取刷新间隔
    /// 2. 计算每个网络接口的收发速率（字节/秒）
    /// 3. 统计 UDP 和 TCP 连接数
    ///
    /// 返回包含网络接口数据以及 UDP/TCP 连接数的结构体。
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

        // calc_connections() 在 Linux 走 netlink、Windows 走 netstat2，
        // 均为同步阻塞 I/O（每秒 4 次 socket/sendto/recvfrom 或全表枚举），
        // 不能直接在 tokio worker 上跑，卸到 blocking 池避免拖慢动态监控热路径。
        // panic 时 unwrap_or_default 退化为 (0,0)，与原先内部错误兜底语义一致。
        let (udp_connections, tcp_connections) =
            tokio::task::spawn_blocking(calc_connections)
                .await
                .unwrap_or_default();

        Self(DynamicNetworkData {
            interfaces: Arc::new(network_vec),
            udp_connections,
            tcp_connections,
        })
    }
}
