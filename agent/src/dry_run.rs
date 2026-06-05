//! Dry-run 模块。
//!
//! 以 `--dry-run` 启动时，采集一轮静态与动态监控数据并通过日志输出，
//! 不建立任何服务器连接。用于验证 agent 能否在本机正常采集数据。

use crate::config_access::get_agent_config;
use crate::monitoring::impls::Monitor;
use log::info;
use ng_monitoring::data_structure::{
    DynamicMonitoringData, StaticMonitoringData, is_excluded_mount, is_virtual_interface,
};

/// 采集并打印静态与动态监控数据快照。
///
/// 1. 采集静态数据（CPU 型号、系统信息、GPU 信息）
/// 2. 采集动态数据（CPU 使用率、内存、负载、磁盘、网络、GPU 实时状态）
/// 3. 输出经 `is_excluded_mount` / `is_virtual_interface` 过滤后的摘要数据
pub async fn dry_run() {
    let static_info = StaticMonitoringData::refresh_and_get().await;
    let dynamic_info = DynamicMonitoringData::refresh_and_get().await;

    info!("DRY-RUN Result");

    info!("STATIC MONITORING DATA");
    info!("CPU:");
    info!("  Logical Cores: {}", static_info.cpu.logical_cores);
    info!("  Physical Cores: {}", static_info.cpu.physical_cores);
    info!("  Per Core:");
    for core in static_info.cpu.per_core {
        info!(
            "    ID: {}, Vendor ID: {}, Name: {}, Brand: {}, ",
            core.id, core.vendor_id, core.name, core.brand
        );
    }

    info!("System:");
    info!("  Host Name: {}", static_info.system.system_host_name);
    info!("  System Name: {}", static_info.system.system_name);
    info!("  Kernel: {}", static_info.system.system_kernel);
    info!(
        "  Kernel Version: {}",
        static_info.system.system_kernel_version
    );
    info!("  OS Version: {}", static_info.system.system_os_version);
    info!(
        "  OS Long Version: {}",
        static_info.system.system_os_long_version
    );
    info!("  Distribution ID: {}", static_info.system.distribution_id);
    info!("  Architecture: {}", static_info.system.arch);
    info!("  Virtualization: {}", static_info.system.virtualization);

    info!("GPU:");
    for gpu in static_info.gpu {
        info!(
            "  ID: {}, Name: {}, Architecture: {}, Cuda Cores: {}",
            gpu.id, gpu.name, gpu.architecture, gpu.cuda_cores
        );
    }

    info!("");

    info!("DYNAMIC MONITORING DATA");
    info!("CPU:");
    info!("  Total Usage: {:.2}%", dynamic_info.cpu.total_cpu_usage);
    info!("  Per Core Usage:");
    for core in dynamic_info.cpu.per_core {
        info!(
            "    ID: {}, Usage: {:.2}%, Frequency: {} mHz",
            core.id, core.cpu_usage, core.frequency_mhz
        );
    }

    info!("Memory:");
    info!(
        "  Total: {} MB ({} Bytes)",
        dynamic_info.ram.total_memory / 1024 / 1024,
        dynamic_info.ram.total_memory
    );
    info!(
        "  Used: {} MB ({} Bytes)",
        dynamic_info.ram.used_memory / 1024 / 1024,
        dynamic_info.ram.used_memory
    );
    info!(
        "  Available: {} MB ({} Bytes)",
        dynamic_info.ram.available_memory / 1024 / 1024,
        dynamic_info.ram.available_memory
    );
    info!(
        "  Total Swap: {} MB ({} Bytes)",
        dynamic_info.ram.total_swap / 1024 / 1024,
        dynamic_info.ram.total_swap
    );
    info!(
        "  Used Swap: {} MB ({} Bytes)",
        dynamic_info.ram.used_swap / 1024 / 1024,
        dynamic_info.ram.used_swap
    );

    info!("Load:");
    info!("  Load Average (1 min): {:.2}", dynamic_info.load.one);
    info!("  Load Average (5 min): {:.2}", dynamic_info.load.five);
    info!("  Load Average (15 min): {:.2}", dynamic_info.load.fifteen);

    info!("Disk:");
    for disk in &dynamic_info.disk {
        info!(
            "  Name: {}, File System: {}, Mount Point: {}, Available Space: {} GB ({} Bytes), Total Space: {} GB ({} Bytes)",
            disk.name,
            disk.file_system,
            disk.mount_point,
            disk.available_space / 1024 / 1024 / 1024,
            disk.available_space,
            disk.total_space / 1024 / 1024 / 1024,
            disk.total_space
        );
    }

    info!("Network:");
    info!(
        "  TCP Connections: {}",
        dynamic_info.network.tcp_connections
    );
    info!(
        "  UDP Connections: {}",
        dynamic_info.network.udp_connections
    );
    info!("  Interfaces:");
    for interface in &dynamic_info.network.interfaces {
        info!(
            "  Name: {}, Total Received: {} MB, Total Transmitted: {} MB",
            interface.interface_name,
            interface.total_received / 1024 / 1024,
            interface.total_transmitted / 1024 / 1024
        );
    }

    info!("GPU:");
    for gpu in dynamic_info.gpu {
        info!(
            "  ID: {}, Used Memory: {} MB ({} Bytes), Total Memory: {} MB ({} Bytes), Graphics Clock: {} MHz, SM Clock: {} MHz, Memory Clock: {} MHz, Video Clock: {} MHz, GPU Utilization: {}%, Memory Utilization: {}%, Temperature: {} °C",
            gpu.id,
            gpu.used_memory / 1024 / 1024,
            gpu.used_memory,
            gpu.total_memory / 1024 / 1024,
            gpu.total_memory,
            gpu.graphics_clock_mhz,
            gpu.sm_clock_mhz,
            gpu.memory_clock_mhz,
            gpu.video_clock_mhz,
            gpu.utilization_gpu,
            gpu.utilization_memory,
            gpu.temperature
        );
    }

    info!("");

    info!("SUMMARY READ DATA SOURCE");
    let cfg = get_agent_config().ok();
    let select_disk = cfg
        .as_ref()
        .and_then(|c| c.dynamic_summary_select_disk.as_deref());
    let select_nic = cfg
        .as_ref()
        .and_then(|c| c.dynamic_summary_select_network_interface.as_deref());
    info!("Disk:");
    let disks: Vec<_> = match select_disk {
        Some(filter) if !filter.is_empty() => dynamic_info
            .disk
            .iter()
            .filter(|d| filter.contains(&d.mount_point))
            .collect(),
        _ => dynamic_info
            .disk
            .iter()
            .filter(|d| !is_excluded_mount(&d.mount_point))
            .collect(),
    };
    for disk in disks {
        info!(
            "  Name: {}, File System: {}, Mount Point: {}, Available Space: {} GB ({} Bytes), Total Space: {} GB ({} Bytes)",
            disk.name,
            disk.file_system,
            disk.mount_point,
            disk.available_space / 1024 / 1024 / 1024,
            disk.available_space,
            disk.total_space / 1024 / 1024 / 1024,
            disk.total_space
        );
    }

    info!("Network:");
    let interfaces: Vec<_> = match select_nic {
        Some(filter) if !filter.is_empty() => dynamic_info
            .network
            .interfaces
            .iter()
            .filter(|i| filter.contains(&i.interface_name))
            .collect(),
        _ => dynamic_info
            .network
            .interfaces
            .iter()
            .filter(|i| !is_virtual_interface(&i.interface_name))
            .collect(),
    };
    for interface in interfaces {
        info!(
            "  Name: {}, Total Received: {} MB, Total Transmitted: {} MB",
            interface.interface_name,
            interface.total_received / 1024 / 1024,
            interface.total_transmitted / 1024 / 1024
        );
    }
}
