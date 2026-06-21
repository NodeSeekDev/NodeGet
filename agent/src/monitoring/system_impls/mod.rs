//! 系统监控数据采集模块。
//!
//! 负责采集 CPU、内存、负载、系统信息等核心监控数据，
//! 分为静态数据（[`StaticDataFromSystem`]）和动态数据（[`DynamicDataFromSystem`]）两类。
//! 子模块 `process` 提供进程数统计，`virtualization_detect` 提供虚拟化环境检测。

use crate::monitoring::refresh_global_system;
use ng_monitoring::data_structure::{
    DynamicCPUData, DynamicLoadData, DynamicPerCpuCoreData, DynamicRamData, DynamicSystemData,
    StaticCPUData, StaticPerCpuCoreData, StaticSystemData,
};
use process::count_processes;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use sysinfo::System;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use virtualization_detect::detect_virtualization;

/// 进程数采集间隔（秒）。
///
/// 进程数是缓变量，1s 与 `PROCESS_REFRESH_INTERVAL_SECS` 秒的分辨率差异对监控可忽略，
/// 但 `/proc` 遍历（Linux）或 `EnumProcesses`（Windows）开销显著，独立低频 ticker
/// 把它从 dynamic tick（1s）解耦，避免每秒遍历全部 PID。
const PROCESS_REFRESH_INTERVAL_SECS: u64 = 5;

/// 全局缓存的进程数，由独立后台 ticker 周期更新，dynamic tick 直接读取（无采集开销）。
static PROCESS_COUNT_CACHE: AtomicU64 = AtomicU64::new(0);

/// 保证进程数采集 ticker 只启动一次。
static PROCESS_TICKER_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// 启动独立后台协程周期性采集进程数（默认每 `PROCESS_REFRESH_INTERVAL_SECS` 秒）。
///
/// 首次立即采集填充缓存，避免 dynamic tick 读到 0。应在 agent 启动时调用一次。
pub fn init_process_count_ticker() {
    if PROCESS_TICKER_STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        // 首次立即采集
        refresh_process_count().await;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            PROCESS_REFRESH_INTERVAL_SECS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await; // 消费首次立即触发
        loop {
            interval.tick().await;
            refresh_process_count().await;
        }
    });
}

/// 采集一次进程数写入全局缓存（卸到 blocking 池）。
async fn refresh_process_count() {
    let count = tokio::task::spawn_blocking(count_processes)
        .await
        .unwrap_or(0);
    PROCESS_COUNT_CACHE.store(u64::from(count), Ordering::Relaxed);
}

/// 读取缓存的进程数（最近一次采集值，无 `/proc` 遍历开销）。
///
/// 注：dynamic summary 上报的 `process_count` 是最近一次 ticker 刷新值
/// （最多滞后 `PROCESS_REFRESH_INTERVAL_SECS` 秒），而非精确到当前秒。
fn cached_process_count() -> u64 {
    PROCESS_COUNT_CACHE.load(Ordering::Relaxed)
}

/// 获取精确的 OS 版本号。
///
/// 优先使用 sysinfo 的 `os_version()`（读取 `/etc/os-release` 的 `VERSION_ID`），
/// 但某些发行版（如 Debian）的 `VERSION_ID` 只有主版本号（如 "11"），
/// 需要 fallback 到发行版专属文件获取小版本号（如 "11.1"）。
///
/// 采用 `tokio::fs` 的异步读取，避免在 tokio runtime worker 上做 blocking IO；
/// `/etc/*` 本地文件几乎都是内存页缓存命中，开销极小。
///
/// 非 Linux 平台上函数体里没有 `.await`（Linux 专属分支被 `cfg` 剔除），clippy 会
/// 误报 `unused_async`；保留 async 签名便于调用点统一（跨平台 `.await` 写法相同）。
#[cfg_attr(not(target_os = "linux"), allow(clippy::unused_async))]
async fn get_precise_os_version() -> String {
    let version = System::os_version().unwrap_or_default();

    #[cfg(target_os = "linux")]
    {
        let distro = System::distribution_id();
        // Debian: /etc/debian_version 包含精确版本号（如 "11.1"）
        if distro == "debian" {
            if let Ok(v) = tokio::fs::read_to_string("/etc/debian_version").await {
                let v = v.trim();
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
        // Alpine: /etc/alpine-release 包含精确版本号（如 "3.18.4"）
        if distro == "alpine" {
            if let Ok(v) = tokio::fs::read_to_string("/etc/alpine-release").await {
                let v = v.trim();
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
        // RHEL/CentOS: /etc/redhat-release 包含 "... release X.Y ..."
        if distro == "rhel" || distro == "centos" || distro == "rocky" || distro == "almalinux" {
            if let Ok(content) = tokio::fs::read_to_string("/etc/redhat-release").await {
                // 格式: "CentOS Linux release 7.9.2009 (Core)"
                if let Some(pos) = content.find("release ") {
                    let after = &content[pos + 8..];
                    let ver: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    if !ver.is_empty() {
                        return ver;
                    }
                }
            }
        }
    }

    version
}

/// 进程数统计模块
pub mod process;
/// 虚拟化环境检测模块
pub mod virtualization_detect;

/// 从系统获取的静态数据结构，包含 CPU 和系统的基本信息。
#[derive(Debug)]
pub struct StaticDataFromSystem(pub StaticCPUData, pub StaticSystemData);

/// 全局静态系统数据实例，用于缓存系统静态信息。
static GLOBAL_STATIC_DATA_FROM_SYSTEM: OnceCell<Mutex<StaticDataFromSystem>> =
    OnceCell::const_new();

impl StaticDataFromSystem {
    /// 创建新的静态系统数据实例。
    ///
    /// 1. 刷新全局系统信息
    /// 2. 获取每核 CPU 静态信息（名称、厂商、品牌）
    /// 3. 获取系统静态信息（内核、架构、虚拟化等）
    ///
    /// 返回包含 CPU 和系统静态数据的结构体。
    pub async fn new() -> Self {
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let (per_core, logical_cores) = {
            let system = system_mutex.lock().await;

            let per_core = system
                .cpus()
                .iter()
                .enumerate()
                .map(|(i, cpu)| StaticPerCpuCoreData {
                    id: (i + 1) as u32,
                    name: cpu.name().to_string(),
                    vendor_id: cpu.vendor_id().to_string(),
                    brand: cpu.brand().trim().to_string(),
                })
                .collect::<Vec<_>>();

            let logical_cores = per_core.len() as u64;
            (per_core, logical_cores)
        };

        Self(
            StaticCPUData {
                physical_cores: System::physical_core_count().unwrap_or(0) as u64,
                logical_cores,
                per_core,
            },
            StaticSystemData {
                system_name: System::name().unwrap_or_default(),
                system_kernel: System::kernel_version().unwrap_or_default(),
                system_kernel_version: System::long_os_version().unwrap_or_default(),
                system_os_version: get_precise_os_version().await,
                system_os_long_version: System::long_os_version().unwrap_or_default(),
                distribution_id: System::distribution_id(),
                system_host_name: System::host_name().unwrap_or_default(),
                arch: System::cpu_arch(),
                virtualization: detect_virtualization().await,
            },
        )
    }

    /// 获取静态系统数据的可变引用。
    ///
    /// 如果全局静态系统数据实例不存在，则初始化它；否则直接返回现有的实例。
    ///
    /// 返回静态系统数据的 `MutexGuard`。
    pub async fn get() -> MutexGuard<'static, Self> {
        let data_mutex = GLOBAL_STATIC_DATA_FROM_SYSTEM
            .get_or_init(|| async { Mutex::new(Self::new().await) })
            .await;

        data_mutex.lock().await
    }
}

/// 从系统获取的动态数据结构，包含 CPU、内存、负载和系统实时数据。
#[derive(Debug)]
pub struct DynamicDataFromSystem(
    pub DynamicCPUData,
    pub DynamicRamData,
    pub DynamicLoadData,
    pub DynamicSystemData,
);
/// 全局动态系统数据实例，用于缓存系统动态信息。
static GLOBAL_DYNAMIC_DATA_FROM_SYSTEM: OnceCell<Mutex<DynamicDataFromSystem>> =
    OnceCell::const_new();

impl DynamicDataFromSystem {
    /// 创建新的动态系统数据实例。
    ///
    /// 1. 刷新全局系统信息
    /// 2. 获取每核 CPU 使用率和频率
    /// 3. 获取内存和 Swap 使用情况
    /// 4. 获取负载均值
    /// 5. 获取启动时间、运行时间和进程数
    ///
    /// 返回包含 CPU、内存、负载和系统动态数据的结构体。
    async fn new() -> Self {
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock().await;

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

        Self(
            DynamicCPUData {
                per_core: Arc::new(per_core),
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
                process_count: cached_process_count(),
            },
        )
    }

    /// 更新动态系统数据。
    ///
    /// 刷新现有系统数据，更新 CPU 使用率和频率、内存使用情况、负载和系统信息。
    async fn update(&mut self) {
        // 仅处理变更数据
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock().await;

        // 构建新的 per_core Vec，避免通过 Arc 修改共享数据
        let new_per_core: Vec<DynamicPerCpuCoreData> = system
            .cpus()
            .iter()
            .enumerate()
            .map(|(id, cpu)| DynamicPerCpuCoreData {
                id: (id + 1) as u32,
                cpu_usage: f64::from(cpu.cpu_usage()),
                frequency_mhz: cpu.frequency(),
            })
            .collect();
        self.0.per_core = Arc::new(new_per_core);
        self.0.total_cpu_usage = f64::from(system.global_cpu_usage());

        self.1.available_memory = system.available_memory();
        self.1.used_memory = system.used_memory();
        self.1.used_swap = system.used_swap();
        self.1.total_memory = system.total_memory();
        self.1.total_swap = system.total_swap();
        drop(system);

        let load = System::load_average();
        self.2.one = load.one;
        self.2.five = load.five;
        self.2.fifteen = load.fifteen;

        self.3.boot_time = System::boot_time();
        self.3.uptime = System::uptime();
        self.3.process_count = cached_process_count();
    }

    /// 异步刷新并获取动态系统数据。
    ///
    /// 如果全局动态系统数据实例不存在，则初始化它；否则更新现有数据并返回。
    ///
    /// 返回动态系统数据的 `MutexGuard`。
    pub async fn refresh_and_get() -> MutexGuard<'static, Self> {
        // 外部调用
        let data_mutex = GLOBAL_DYNAMIC_DATA_FROM_SYSTEM
            .get_or_init(|| async { Mutex::new(Self::new().await) })
            .await;

        let mut data = data_mutex.lock().await;
        data.update().await;

        data
    }
}
