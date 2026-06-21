//! 监控数据采集模块。
//!
//! 维护全局系统信息（CPU、内存、磁盘、网络、GPU）的缓存单例，
//! 并提供带时间间隔追踪的刷新函数，供上报循环按需调用。
//! 子模块 `gpu`、`system_impls`、`network_connections` 分别负责各领域的采集实现，
//! `impls` 模块将它们组合为 [`StaticMonitoringData`] / [`DynamicMonitoringData`]。

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

/// 启动进程数低频采集 ticker（转发至 `system_impls`，默认 5s 间隔）。
///
/// 从 dynamic tick(1s)解耦 `/proc` 遍历，避免每秒采集拖慢动态监控热路径。
/// 内部 `OnceLock` 保证只启动一次。
pub fn init_process_count_ticker() {
    system_impls::init_process_count_ticker();
}

/// 全局系统信息实例（sysinfo `System`），用于获取和刷新 CPU/内存信息。
static GLOBAL_SYSTEM: OnceCell<Mutex<System>> = OnceCell::const_new();

/// 获取全局系统信息实例，如果不存在则初始化。
///
/// 返回指向全局系统信息实例的静态引用。
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

/// 刷新全局系统信息，包括 CPU 使用率、频率以及内存信息。
async fn refresh_global_system() {
    let system_mutex = get_global_system().await;
    {
        let mut system = system_mutex.lock().await;
        system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
    }
}

/// 全局磁盘信息实例（sysinfo `Disks`），用于获取和刷新磁盘信息。
static GLOBAL_DISK: OnceCell<Mutex<Disks>> = OnceCell::const_new();

/// 磁盘刷新时间追踪器，用于计算两次刷新之间的时间间隔，供速率计算使用。
static DISK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

/// 获取全局磁盘信息实例，如果不存在则初始化。
///
/// 返回指向全局磁盘信息实例的静态引用。
async fn get_global_disk() -> &'static Mutex<Disks> {
    GLOBAL_DISK
        .get_or_init(|| async {
            let mut disk = Disks::new();
            disk.refresh(true);
            Mutex::new(disk)
        })
        .await
}

/// 刷新全局磁盘信息并返回刷新间隔。
///
/// 1. 首次初始化时将 tracker 回拨 1 秒，避免首轮 `now - last_time` 接近 0
/// 2. 刷新磁盘 IO 使用率和存储信息
/// 3. 计算与上次刷新的时间间隔
///
/// 返回两次刷新之间的持续时间，供速率计算使用。
async fn refresh_global_disk() -> Duration {
    // 首次初始化时把 tracker 回调一秒，避免首轮 `now - last_time` 接近 0，
    // 进而让下游速率计算（bytes / interval_secs）拿到可用于推导的分母。
    // checked_sub 防御 monotonic clock 刚启动不满 1s 的极端场景。
    let time_tracker = DISK_TIME_TRACKER
        .get_or_init(|| async {
            Mutex::new(
                Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
            )
        })
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

/// 全局网络信息实例（sysinfo `Networks`），用于获取和刷新网络接口信息。
static GLOBAL_NETWORK: OnceCell<Mutex<Networks>> = OnceCell::const_new();

/// 网络刷新时间追踪器，用于计算两次刷新之间的时间间隔，供速率计算使用。
static NETWORK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

/// 获取全局网络信息实例，如果不存在则初始化。
///
/// 返回指向全局网络信息实例的静态引用。
async fn get_global_network() -> &'static Mutex<Networks> {
    GLOBAL_NETWORK
        .get_or_init(|| async {
            let mut network = Networks::new();
            network.refresh(true);
            Mutex::new(network)
        })
        .await
}

/// 刷新全局网络信息并返回刷新间隔。
///
/// 1. 首次初始化时将 tracker 回拨 1 秒，确保首轮 interval 不为零
/// 2. 刷新网络接口统计信息
/// 3. 计算与上次刷新的时间间隔
///
/// 返回两次刷新之间的持续时间，供速率计算使用。
async fn refresh_global_network() -> Duration {
    // 与 `refresh_global_disk` 对齐：首次初始化回拨 1s，确保首轮 interval 不为零。
    let time_tracker = NETWORK_TIME_TRACKER
        .get_or_init(|| async {
            Mutex::new(
                Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
            )
        })
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

/// 全局 GPU 信息实例（NVML），用于获取和初始化 NVIDIA Management Library。
/// 无 NVIDIA 驱动时为 `None`。
static GLOBAL_GPU: OnceCell<Mutex<Option<Nvml>>> = OnceCell::const_new();

/// 获取全局 GPU 信息实例，如果不存在则尝试初始化 NVML。
///
/// 返回指向全局 GPU 信息实例的静态引用；NVML 不可用时内部值为 `None`。
async fn get_global_gpu() -> &'static Mutex<Option<Nvml>> {
    GLOBAL_GPU
        .get_or_init(|| async {
            let nvml = Nvml::init().ok();
            Mutex::new(nvml)
        })
        .await
}
