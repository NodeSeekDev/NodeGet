use nvml_wrapper::Nvml;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, System};
use tokio::sync::{Mutex, OnceCell};
use tokio::time::Instant;

// GPU 监控模块
mod gpu;
// 监控实现模块
pub mod impls;
// 网络连接监控模块
mod network_connections;
// 系统实现模块
mod system_impls;
// 系统监控相关功能

// 全局系统信息实例，用于获取和刷新系统信息
static GLOBAL_SYSTEM: OnceCell<Mutex<System>> = OnceCell::const_new();

// 获取全局系统信息实例，如果不存在则初始化
//
// # 返回值
// 返回指向全局系统信息实例的静态引用
async fn get_global_system() -> &'static Mutex<System> {
    GLOBAL_SYSTEM
        .get_or_init(|| async {
            let mut system = System::new();
            system.refresh_cpu_all();
            system.refresh_memory();
            Mutex::new(system)
        })
        .await
}

// 刷新全局系统信息，包括CPU和内存信息
//
// 该函数获取全局系统实例并刷新CPU使用率、频率以及内存信息
async fn refresh_global_system() {
    let system_mutex = get_global_system().await;
    {
        let mut system = system_mutex.lock().await;
        system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
    }
}

// 磁盘监控相关功能

// 全局磁盘信息实例，用于获取和刷新磁盘信息
static GLOBAL_DISK: OnceCell<Mutex<Disks>> = OnceCell::const_new();

// 磁盘刷新时间追踪器，用于计算两次刷新之间的时间间隔
static DISK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

// 获取全局磁盘信息实例，如果不存在则初始化
//
// # 返回值
// 返回指向全局磁盘信息实例的静态引用
async fn get_global_disk() -> &'static Mutex<Disks> {
    GLOBAL_DISK
        .get_or_init(|| async {
            let mut disk = Disks::new();
            disk.refresh(true);
            Mutex::new(disk)
        })
        .await
}

// 刷新全局磁盘信息并返回刷新间隔
//
// 该函数刷新磁盘IO使用情况和存储信息，并计算与上次刷新的时间间隔
//
// # 返回值
// 返回两次刷新之间的持续时间
async fn refresh_global_disk() -> Duration {
    let time_tracker = DISK_TIME_TRACKER
        .get_or_init(|| async { Mutex::new(Instant::now()) })
        .await;

    let disk_mutex = get_global_disk().await;
    {
        let mut disk = disk_mutex.lock().await;
        disk.refresh_specifics(
            true,
            DiskRefreshKind::nothing()
                .with_io_usage()
                .with_storage()
                .without_kind(),
        );

        let mut last_time = time_tracker.lock().await;
        let now = Instant::now();
        let interval = now.duration_since(*last_time);

        *last_time = now;
        interval
    }
}

// 网络监控相关功能

// 全局网络信息实例，用于获取和刷新网络接口信息
static GLOBAL_NETWORK: OnceCell<Mutex<Networks>> = OnceCell::const_new();

// 网络刷新时间追踪器，用于计算两次刷新之间的时间间隔
static NETWORK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

// 获取全局网络信息实例，如果不存在则初始化
//
// # 返回值
// 返回指向全局网络信息实例的静态引用
async fn get_global_network() -> &'static Mutex<Networks> {
    GLOBAL_NETWORK
        .get_or_init(|| async {
            let mut network = Networks::new();
            network.refresh(true);
            Mutex::new(network)
        })
        .await
}

// 刷新全局网络信息并返回刷新间隔
//
// 该函数刷新网络接口信息，并计算与上次刷新的时间间隔
//
// # 返回值
// 返回两次刷新之间的持续时间
async fn refresh_global_network() -> Duration {
    let time_tracker = NETWORK_TIME_TRACKER
        .get_or_init(|| async { Mutex::new(Instant::now()) })
        .await;

    let network_mutex = get_global_network().await;
    {
        let mut network = network_mutex.lock().await;
        network.refresh(true);

        let mut last_time = time_tracker.lock().await;
        let now = Instant::now();
        let interval = now.duration_since(*last_time);
        *last_time = now;
        interval
    }
}

// GPU 监控相关功能

// 全局GPU信息实例，用于获取和初始化NVML（NVIDIA Management Library）
static GLOBAL_GPU: OnceCell<Mutex<Option<Nvml>>> = OnceCell::const_new();

// 获取全局GPU信息实例，如果不存在则尝试初始化NVML
//
// # 返回值
// 返回指向全局GPU信息实例（可能包含NVML对象或为None）的静态引用
async fn get_global_gpu() -> &'static Mutex<Option<Nvml>> {
    GLOBAL_GPU
        .get_or_init(|| async {
            let nvml = Nvml::init().ok();
            Mutex::new(nvml)
        })
        .await
}
