use parking_lot::Mutex;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, System};
use tokio::sync::OnceCell;
use tokio::time::Instant;

pub mod data_structure;
pub mod impls;
mod network_connections;
mod process;
mod virtualization_detect;
// System

static GLOBAL_SYSTEM: OnceCell<Mutex<System>> = OnceCell::const_new();

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

async fn refresh_global_system() {
    let system_mutex = get_global_system().await;
    let mut system = system_mutex.lock();
    system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
    system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
}

// Disk

static GLOBAL_DISK: OnceCell<Mutex<Disks>> = OnceCell::const_new();

static DISK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

async fn get_global_disk() -> &'static Mutex<Disks> {
    GLOBAL_DISK
        .get_or_init(|| async {
            let mut disk = Disks::new();
            disk.refresh(true);
            Mutex::new(disk)
        })
        .await
}

async fn refresh_global_disk() -> Duration {
    let time_tracker = DISK_TIME_TRACKER
        .get_or_init(|| async { Mutex::new(Instant::now()) })
        .await;

    let disk_mutex = get_global_disk().await;
    let mut disk = disk_mutex.lock();
    disk.refresh_specifics(
        true,
        DiskRefreshKind::nothing()
            .with_io_usage()
            .with_storage()
            .without_kind(),
    );

    let mut last_time = time_tracker.lock();
    let now = Instant::now();
    let interval = now.duration_since(*last_time);

    *last_time = now;
    interval
}

// Network

static GLOBAL_NETWORK: OnceCell<Mutex<Networks>> = OnceCell::const_new();

static NETWORK_TIME_TRACKER: OnceCell<Mutex<Instant>> = OnceCell::const_new();

async fn get_global_network() -> &'static Mutex<Networks> {
    GLOBAL_NETWORK
        .get_or_init(|| async {
            let mut network = Networks::new();
            network.refresh(true);
            Mutex::new(network)
        })
        .await
}

async fn refresh_global_network() -> Duration {
    let time_tracker = NETWORK_TIME_TRACKER
        .get_or_init(|| async { Mutex::new(Instant::now()) })
        .await;

    let network_mutex = get_global_network().await;
    let mut network = network_mutex.lock();
    network.refresh(true);

    let mut last_time = time_tracker.lock();
    let now = Instant::now();
    let interval = now.duration_since(*last_time);

    *last_time = now;
    interval
}
