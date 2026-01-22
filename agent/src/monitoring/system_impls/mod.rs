// System

use crate::monitoring::refresh_global_system;
use nodeget_lib::monitoring::data_structure::{
    DynamicCPUData, DynamicLoadData, DynamicPerCpuCoreData, DynamicRamData, DynamicSystemData,
    StaticCPUData, StaticPerCpuCoreData, StaticSystemData,
};
use process::count_processes;
use sysinfo::System;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use virtualization_detect::detect_virtualization;

pub mod process;
pub mod virtualization_detect;

#[derive(Debug)]
pub struct StaticDataFromSystem(pub StaticCPUData, pub StaticSystemData);

static GLOBAL_STATIC_DATA_FROM_SYSTEM: OnceCell<Mutex<StaticDataFromSystem>> =
    OnceCell::const_new();

impl StaticDataFromSystem {
    pub async fn new() -> Self {
        refresh_global_system().await;
        let system_mutex = crate::monitoring::get_global_system().await;
        let system = system_mutex.lock().await;

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
                system_os_version: System::os_version().unwrap_or_default(),
                system_os_long_version: System::long_os_version().unwrap_or_default(),
                distribution_id: System::distribution_id(),
                system_host_name: System::host_name().unwrap_or_default(),
                arch: System::cpu_arch(),
                virtualization: detect_virtualization().await,
            },
        )
    }

    pub async fn get() -> MutexGuard<'static, Self> {
        let data_mutex = GLOBAL_STATIC_DATA_FROM_SYSTEM
            .get_or_init(|| async { Mutex::new(Self::new().await) })
            .await;

        data_mutex.lock().await
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
        let system = system_mutex.lock().await;

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
