//! 监控数据结构体定义。
//!
//! 定义了三类监控数据（静态、动态、动态摘要）及其子结构体，
//! 以及相关的辅助工具（哈希计算、虚拟接口/排除挂载点判断、缩放转换）。
//! 若数据量字段中未注明单位，则以字节（Bytes）为单位。

use sha2::{Digest, Sha256};

/// 静态监控数据，包含不会随时间变化的硬件信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticMonitoringData {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub time: u64,
    /// 数据内容的 SHA-256 哈希（前 16 字节原始二进制），用于去重
    pub data_hash: Vec<u8>,

    /// CPU 静态信息
    pub cpu: StaticCPUData,
    /// 系统静态信息
    pub system: StaticSystemData,
    /// GPU 静态信息列表
    pub gpu: Vec<StaticGpuData>,
}

/// 将 u64 安全饱和转换为 i64，超过 `i64::MAX` 时返回 `i64::MAX`，避免静默回绕。
#[must_use]
fn u64_to_i64_saturating(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// 将 u64 安全饱和转换为 i32，超过 `i32::MAX` 时返回 `i32::MAX`，避免静默回绕。
#[must_use]
fn u64_to_i32_saturating(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

impl StaticMonitoringData {
    /// 根据 cpu / system / gpu 三个字段的内容计算确定性 SHA-256 哈希。
    ///
    /// 内部将三个字段各自序列化为 `serde_json::Value`，再递归排序所有 object key，
    /// 以确定性顺序直接写入 SHA-256 hasher，零中间分配。
    /// 同一组数据无论 JSON 序列化时 key 顺序如何，都会得到相同的哈希值。
    ///
    /// # Errors
    ///
    /// 当任何字段的序列化失败时返回 `serde_json::Error`。
    /// 对于仅包含标准可序列化类型的结构体，此情况在实际上不会发生。
    ///
    /// - `cpu` — CPU 静态信息
    /// - `system` — 系统静态信息
    /// - `gpu` — GPU 静态信息列表
    /// - 返回值 — 前 16 字节（128 bit）的 SHA-256 哈希摘要
    pub fn compute_data_hash(
        cpu: &StaticCPUData,
        system: &StaticSystemData,
        gpu: &[StaticGpuData],
    ) -> Result<Vec<u8>, ng_core::error::NodegetError> {
        use std::io::Write;

        let cpu_val = serde_json::to_value(cpu).map_err(ng_core::error::NodegetError::from)?;
        let sys_val = serde_json::to_value(system).map_err(ng_core::error::NodegetError::from)?;
        let gpu_val = serde_json::to_value(gpu).map_err(ng_core::error::NodegetError::from)?;

        let mut hasher = Sha256::new();
        let mut writer = WriteToDigest(&mut hasher);
        write_canonical_json(&cpu_val, &mut writer).map_err(|e| {
            ng_core::error::NodegetError::Other(format!("canonical write failed: {e}"))
        })?;
        writer.write_all(b"\n").map_err(|e| {
            ng_core::error::NodegetError::Other(format!("canonical write failed: {e}"))
        })?;
        write_canonical_json(&sys_val, &mut writer).map_err(|e| {
            ng_core::error::NodegetError::Other(format!("canonical write failed: {e}"))
        })?;
        writer.write_all(b"\n").map_err(|e| {
            ng_core::error::NodegetError::Other(format!("canonical write failed: {e}"))
        })?;
        write_canonical_json(&gpu_val, &mut writer).map_err(|e| {
            ng_core::error::NodegetError::Other(format!("canonical write failed: {e}"))
        })?;

        let hash = hasher.finalize();
        // 取前 16 字节 (128 bit) 足够去重
        Ok(hash[..16].to_vec())
    }
}

/// 将 `std::io::Write` 调用桥接到 `Sha256::update`，实现零分配流式哈希。
struct WriteToDigest<'a>(&'a mut Sha256);

impl std::io::Write for WriteToDigest<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        sha2::Digest::update(self.0, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 将 `serde_json::Value` 以确定性顺序直接写入 Writer，零 clone。
///
/// 对于 Object，按 key 的字典序排序后递归写入；
/// 对于 Array，按原有顺序递归写入；
/// 对于标量，通过 `serde_json::to_writer` 序列化。
fn write_canonical_json<W: std::io::Write>(
    v: &serde_json::Value,
    w: &mut W,
) -> std::io::Result<()> {
    match v {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(std::string::String::as_str).collect();
            keys.sort_unstable();
            w.write_all(b"{")?;
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    w.write_all(b",")?;
                }
                serde_json::to_writer(&mut *w, k)?;
                w.write_all(b":")?;
                write_canonical_json(map.get(*k).unwrap(), w)?;
            }
            w.write_all(b"}")?;
        }
        serde_json::Value::Array(arr) => {
            w.write_all(b"[")?;
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    w.write_all(b",")?;
                }
                write_canonical_json(v, w)?;
            }
            w.write_all(b"]")?;
        }
        other => {
            serde_json::to_writer(w, other)?;
        }
    }
    Ok(())
}

/// 动态监控数据，包含随时间变化的系统状态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringData {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub time: u64,

    /// CPU 动态信息
    pub cpu: DynamicCPUData,
    /// 内存动态信息
    pub ram: DynamicRamData,
    /// 系统负载动态信息
    pub load: DynamicLoadData,
    /// 系统动态信息
    pub system: DynamicSystemData,
    /// 磁盘动态信息列表
    pub disk: Vec<DynamicPerDiskData>,
    /// 网络动态信息
    pub network: DynamicNetworkData,
    /// GPU 动态信息列表
    pub gpu: Vec<DynamicGpuData>,
}

/// 动态监控摘要数据，包含扁平化的系统状态摘要信息。
///
/// 字段均为 `Option` 以应对 Agent 采集缺失的情况。
/// 缩放字段（`cpu_usage`、`load_*`）在写入时乘以 10 存储，读取时需除以 10.0 还原。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringSummaryData {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub time: u64,

    /// CPU 使用率百分比，缩放存储（实际值 = 存储值 / 10.0）
    pub cpu_usage: Option<i16>,
    /// GPU 使用率百分比（0-100）
    pub gpu_usage: Option<i16>,
    /// 已用交换空间（字节）
    pub used_swap: Option<i64>,
    /// 总交换空间（字节）
    pub total_swap: Option<i64>,
    /// 已用内存（字节）
    pub used_memory: Option<i64>,
    /// 总内存（字节）
    pub total_memory: Option<i64>,
    /// 可用内存（字节）
    pub available_memory: Option<i64>,
    /// 1 分钟平均负载，缩放存储（实际值 = 存储值 / 10.0）
    pub load_one: Option<i16>,
    /// 5 分钟平均负载，缩放存储（实际值 = 存储值 / 10.0）
    pub load_five: Option<i16>,
    /// 15 分钟平均负载，缩放存储（实际值 = 存储值 / 10.0）
    pub load_fifteen: Option<i16>,
    /// 系统运行时间（秒）
    pub uptime: Option<i32>,
    /// 系统启动时间（秒时间戳）
    pub boot_time: Option<i64>,
    /// 进程数量
    pub process_count: Option<i32>,
    /// 磁盘总空间（字节）
    pub total_space: Option<i64>,
    /// 磁盘可用空间（字节）
    pub available_space: Option<i64>,
    /// 磁盘读取速度（字节/秒）
    pub read_speed: Option<i64>,
    /// 磁盘写入速度（字节/秒）
    pub write_speed: Option<i64>,
    /// TCP 连接数
    pub tcp_connections: Option<i32>,
    /// UDP 连接数
    pub udp_connections: Option<i32>,
    /// 网络总接收量（字节）
    pub total_received: Option<i64>,
    /// 网络总发送量（字节）
    pub total_transmitted: Option<i64>,
    /// 网络发送速度（字节/秒）
    pub transmit_speed: Option<i64>,
    /// 网络接收速度（字节/秒）
    pub receive_speed: Option<i64>,
}

/// 虚拟网卡前缀列表，匹配这些前缀的接口在摘要统计中被排除。
const VIRTUAL_INTERFACE_PREFIXES: &[&str] = &[
    "br", "cni", "docker", "podman", "flannel", "lo", "veth", "virbr", "vmbr", "tap", "fwbr",
    "fwpr",
];

/// 排除的挂载点前缀列表，匹配这些前缀的磁盘在摘要统计中被排除。
const EXCLUDED_MOUNT_PREFIXES: &[&str] = &[
    "/tmp",
    "/var/tmp",
    "/dev",
    "/run",
    "/var/lib/containerd",
    "/var/lib/containers",
    "/var/lib/docker",
    "/var/lib/kubelet/plugins",
    "/var/lib/kubelet/plugins_registry",
    "/var/lib/kubelet/pods",
    "/var/lib/rancher/k3s/agent/containerd",
    "/var/lib/rancher/k3s/agent/kubelet/plugins",
    "/var/lib/rancher/k3s/agent/kubelet/plugins_registry",
    "/var/lib/rancher/k3s/agent/kubelet/pods",
    "/proc",
    "/sys",
    "/sys/fs/cgroup",
    "/etc/resolv.conf",
    "/etc/host",
    "/etc/hostname",
    "/etc/hosts",
    "/nix/store",
];

/// 排除的文件系统类型列表，匹配这些类型的磁盘在摘要统计中被排除。
const EXCLUDED_FILE_SYSTEMS: &[&str] = &[
    "autofs",
    "bpf",
    "cgroup",
    "cgroup2",
    "debugfs",
    "devtmpfs",
    "fusectl",
    "nsfs",
    "overlay",
    "proc",
    "pstore",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

/// 判断网卡名称是否为虚拟接口。
///
/// - `name` — 网卡名称
/// - 返回值 — 若匹配虚拟接口前缀则返回 `true`
#[must_use]
pub fn is_virtual_interface(name: &str) -> bool {
    VIRTUAL_INTERFACE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// 判断挂载点是否匹配路径前缀。
///
/// 只有完整路径段匹配才返回 `true`，避免 `/run` 误匹配 `/runner`。
#[must_use]
fn mount_point_matches_prefix(mount_point: &str, prefix: &str) -> bool {
    mount_point == prefix
        || mount_point
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 判断挂载点是否应被排除（不纳入摘要统计）。
///
/// - `mount_point` — 挂载点路径
/// - 返回值 — 若匹配排除前缀则返回 `true`
#[must_use]
pub fn is_excluded_mount(mount_point: &str) -> bool {
    EXCLUDED_MOUNT_PREFIXES
        .iter()
        .any(|prefix| mount_point_matches_prefix(mount_point, prefix))
}

/// 判断文件系统类型是否应被排除（不纳入摘要统计）。
///
/// - `file_system` — 文件系统类型
/// - 返回值 — 若匹配排除类型则返回 `true`
#[must_use]
pub fn is_excluded_file_system(file_system: &str) -> bool {
    EXCLUDED_FILE_SYSTEMS
        .iter()
        .any(|excluded| file_system.eq_ignore_ascii_case(excluded))
}

/// 判断单块磁盘是否应被排除（不纳入摘要统计）。
///
/// 完整动态数据仍保留所有磁盘；此函数仅用于 Dynamic Summary 的默认汇总口径。
#[must_use]
pub fn is_excluded_summary_disk(disk: &DynamicPerDiskData) -> bool {
    is_excluded_mount(&disk.mount_point) || is_excluded_file_system(&disk.file_system)
}

/// 将百分比数值缩放为 i16 存储格式（乘以 10，保留一位小数精度）。
///
/// 用于 `dynamic_monitoring_summary.cpu_usage` 列，读取时需除以 10.0 还原。
/// 此函数是写入侧唯一强制执行该不变量的位置，并防护两类上游数据损坏：
///
/// * **`NaN` / `±Infinity`** — `f64::clamp` 对 `NaN` 无效，`f64 as i16` 会静默
///   折叠为 0，导致面板显示"0% CPU"。此处返回 `None` 以记录空缺而非伪造零值。
/// * **超范围百分比**（如 sysinfo 在容器首次采样时返回 `> 100.0`）—
///   旧实现允许最大 `i16::MAX = 32767`（即 3276.7%）直接写入数据库。
///   现在将结果 clamp 至 `[0, 1000]`，确保反缩放后始终在 `[0.0, 100.0]` 范围内。
///
/// - `percent` — 原始百分比浮点数
/// - 返回值 — 缩放后的 `i16`（`Some`），或 `None`（输入非有限数）
#[must_use]
fn scale_cpu_percent_to_i16(percent: f64) -> Option<i16> {
    if !percent.is_finite() {
        return None;
    }
    // 10.0 * percent with clamp to [0, 1000] — one decimal place precision,
    // maximum 100.0%. Negative sysinfo values (should never happen but
    // defend anyway) are folded to 0 rather than negative CPU.
    let scaled = (percent * 10.0).clamp(0.0, 1000.0);
    // `scaled` is now a finite f64 in [0, 1000]; `as i16` is lossy only in
    // the fractional bits, which is the intended truncation.
    #[allow(clippy::cast_possible_truncation)]
    let v = scaled as i16;
    Some(v)
}

/// 将 1/5/15 分钟平均负载缩放为 i16 存储格式（乘以 10）。
///
/// 与 CPU 百分比不同，负载平均值在高负载系统上可以合法超过 100（如 256 线程机器上
/// 负载 200）。仍然 clamp 至 `i16` 范围以避免 `as i16` 静默回绕，但上限为
/// `i16::MAX` 而非 `1000`。`NaN` 同样以 `None`（缺失数据）表示，而非折叠为 0。
///
/// - `load` — 原始负载浮点数
/// - 返回值 — 缩放后的 `i16`（`Some`），或 `None`（输入非有限数）
#[must_use]
fn scale_load_to_i16(load: f64) -> Option<i16> {
    if !load.is_finite() {
        return None;
    }
    let scaled = (load * 10.0).clamp(f64::from(i16::MIN), f64::from(i16::MAX));
    #[allow(clippy::cast_possible_truncation)]
    let v = scaled as i16;
    Some(v)
}

impl DynamicMonitoringSummaryData {
    /// 使用可选的磁盘和网卡筛选列表构建 `DynamicMonitoringSummaryData`
    ///
    /// - `select_disk`: 若存在且非空，仅统计 `mount_point` 匹配该列表的磁盘；否则回退到默认排除逻辑
    /// - `select_network_interface`: 若存在且非空，仅统计 `interface_name` 匹配该列表的网卡；否则回退到默认排除逻辑
    #[must_use]
    pub fn from_with_filter(
        data: &DynamicMonitoringData,
        select_disk: Option<&[String]>,
        select_network_interface: Option<&[String]>,
    ) -> Self {
        let disks: Vec<_> = match select_disk {
            Some(filter) if !filter.is_empty() => data
                .disk
                .iter()
                .filter(|d| filter.contains(&d.mount_point))
                .collect(),
            _ => data
                .disk
                .iter()
                .filter(|d| !is_excluded_summary_disk(d))
                .collect(),
        };
        let total_space: u64 = disks.iter().map(|d| d.total_space).sum();
        let available_space: u64 = disks.iter().map(|d| d.available_space).sum();
        let read_speed: u64 = disks.iter().map(|d| d.read_speed).sum();
        let write_speed: u64 = disks.iter().map(|d| d.write_speed).sum();

        let ifaces: Vec<_> = match select_network_interface {
            Some(filter) if !filter.is_empty() => data
                .network
                .interfaces
                .iter()
                .filter(|i| filter.contains(&i.interface_name))
                .collect(),
            _ => data
                .network
                .interfaces
                .iter()
                .filter(|i| !is_virtual_interface(&i.interface_name))
                .collect(),
        };
        let total_received: u64 = ifaces.iter().map(|i| i.total_received).sum();
        let total_transmitted: u64 = ifaces.iter().map(|i| i.total_transmitted).sum();
        let receive_speed_net: u64 = ifaces.iter().map(|i| i.receive_speed).sum();
        let transmit_speed: u64 = ifaces.iter().map(|i| i.transmit_speed).sum();

        Self {
            uuid: data.uuid,
            time: data.time,
            cpu_usage: scale_cpu_percent_to_i16(data.cpu.total_cpu_usage),
            gpu_usage: data.gpu.first().map(|g| i16::from(g.utilization_gpu)),
            used_swap: Some(u64_to_i64_saturating(data.ram.used_swap)),
            total_swap: Some(u64_to_i64_saturating(data.ram.total_swap)),
            used_memory: Some(u64_to_i64_saturating(data.ram.used_memory)),
            total_memory: Some(u64_to_i64_saturating(data.ram.total_memory)),
            available_memory: Some(u64_to_i64_saturating(data.ram.available_memory)),
            load_one: scale_load_to_i16(data.load.one),
            load_five: scale_load_to_i16(data.load.five),
            load_fifteen: scale_load_to_i16(data.load.fifteen),
            uptime: Some(u64_to_i32_saturating(data.system.uptime)),
            boot_time: Some(u64_to_i64_saturating(data.system.boot_time)),
            process_count: Some(u64_to_i32_saturating(data.system.process_count)),
            total_space: Some(u64_to_i64_saturating(total_space)),
            available_space: Some(u64_to_i64_saturating(available_space)),
            read_speed: Some(u64_to_i64_saturating(read_speed)),
            write_speed: Some(u64_to_i64_saturating(write_speed)),
            tcp_connections: Some(u64_to_i32_saturating(data.network.tcp_connections)),
            udp_connections: Some(u64_to_i32_saturating(data.network.udp_connections)),
            total_received: Some(u64_to_i64_saturating(total_received)),
            total_transmitted: Some(u64_to_i64_saturating(total_transmitted)),
            transmit_speed: Some(u64_to_i64_saturating(transmit_speed)),
            receive_speed: Some(u64_to_i64_saturating(receive_speed_net)),
        }
    }
}

impl From<&DynamicMonitoringData> for DynamicMonitoringSummaryData {
    fn from(data: &DynamicMonitoringData) -> Self {
        Self::from_with_filter(data, None, None)
    }
}

/// CPU 静态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticCPUData {
    /// 物理核心数
    pub physical_cores: u64,
    /// 逻辑核心数
    pub logical_cores: u64,
    /// 每个 CPU 核心的静态信息列表
    pub per_core: Vec<StaticPerCpuCoreData>,
}

/// CPU 动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicCPUData {
    /// 每个 CPU 核心的动态信息列表
    pub per_core: Vec<DynamicPerCpuCoreData>,
    /// CPU 总使用率（0-100）
    pub total_cpu_usage: f64,
}

/// 每个 CPU 核心的静态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticPerCpuCoreData {
    /// 核心 ID，从 1 开始
    pub id: u32,
    /// 核心名称
    pub name: String,
    /// 供应商 ID
    pub vendor_id: String,
    /// CPU 品牌
    pub brand: String,
}

/// 每个 CPU 核心的动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerCpuCoreData {
    /// 核心 ID，从 1 开始
    pub id: u32,
    /// CPU 使用率（0-100）
    pub cpu_usage: f64,
    /// CPU 频率（MHz）
    pub frequency_mhz: u64,
}

/// 内存动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicRamData {
    /// 总内存大小（字节）
    pub total_memory: u64,
    /// 可用内存大小（字节）
    pub available_memory: u64,
    /// 已使用内存大小（字节）
    pub used_memory: u64,
    /// 总交换空间大小（字节）
    pub total_swap: u64,
    /// 已使用交换空间大小（字节）
    pub used_swap: u64,
}

/// 系统负载动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicLoadData {
    /// 1 分钟平均负载
    pub one: f64,
    /// 5 分钟平均负载
    pub five: f64,
    /// 15 分钟平均负载
    pub fifteen: f64,
}

/// 系统静态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticSystemData {
    /// 系统名称
    pub system_name: String,
    /// 系统内核版本
    pub system_kernel: String,
    /// 系统内核详细版本
    pub system_kernel_version: String,
    /// 系统操作系统版本
    pub system_os_version: String,
    /// 系统操作系统详细版本
    pub system_os_long_version: String,
    /// 发行版 ID
    pub distribution_id: String,
    /// 系统主机名
    pub system_host_name: String,
    /// 系统架构
    pub arch: String,
    /// 虚拟化平台
    pub virtualization: String,
}

/// 系统动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicSystemData {
    /// 系统启动时间（秒时间戳）
    pub boot_time: u64,
    /// 系统运行时间（秒）
    pub uptime: u64,
    /// 进程数量
    pub process_count: u64,
}

/// 磁盘类型枚举。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum DiskKind {
    /// 机械硬盘
    Hdd,
    /// 固态硬盘
    Ssd,
    /// 未知类型
    Unknown,
}

/// 每个磁盘的动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerDiskData {
    /// 磁盘类型
    pub kind: DiskKind,
    /// 磁盘名称
    pub name: String,
    /// 文件系统类型
    pub file_system: String,
    /// 挂载点
    pub mount_point: String,
    /// 总空间大小（字节）
    pub total_space: u64,
    /// 可用空间大小（字节）
    pub available_space: u64,
    /// 是否可移动
    pub is_removable: bool,
    /// 是否只读
    pub is_read_only: bool,
    /// 读取速度（字节/秒）
    pub read_speed: u64,
    /// 写入速度（字节/秒）
    pub write_speed: u64,
}

/// 网络动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicNetworkData {
    /// 网络接口列表
    pub interfaces: Vec<DynamicPerNetworkInterfaceData>,
    /// UDP 连接数
    pub udp_connections: u64,
    /// TCP 连接数
    pub tcp_connections: u64,
}

/// 每个网络接口的动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerNetworkInterfaceData {
    /// 网络接口名称
    pub interface_name: String,
    /// 总接收数据量（字节），从上次网卡重启开始计算
    pub total_received: u64,
    /// 总发送数据量（字节），从上次网卡重启开始计算
    pub total_transmitted: u64,
    /// 接收速度（字节/秒）
    pub receive_speed: u64,
    /// 发送速度（字节/秒）
    pub transmit_speed: u64,
}

/// GPU 静态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticGpuData {
    /// GPU ID，从 1 开始
    pub id: u32,
    /// GPU 名称
    pub name: String,
    /// CUDA 核心数（对于非 NVIDIA 显卡，该值为 0）
    pub cuda_cores: u64,
    /// GPU 架构
    pub architecture: String,
}

/// GPU 动态信息。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicGpuData {
    /// GPU ID，从 1 开始
    pub id: u32,
    /// 已使用显存（字节）
    pub used_memory: u64,
    /// 总显存（字节）
    pub total_memory: u64,
    /// 图形时钟频率（MHz）
    pub graphics_clock_mhz: u64,
    /// 流处理器时钟频率（MHz），NV: Streaming Multiprocessor；AMD: Compute Unit
    pub sm_clock_mhz: u64,
    /// 显存时钟频率（MHz）
    pub memory_clock_mhz: u64,
    /// 视频时钟频率（MHz）
    pub video_clock_mhz: u64,
    /// GPU 使用率百分比
    pub utilization_gpu: u8,
    /// 显存使用率百分比（不是显存占用率，反映内存读写频率的数值）
    pub utilization_memory: u8,
    /// 温度（摄氏度）
    pub temperature: u8,
}

#[cfg(test)]
mod tests {
    use super::{
        DiskKind, DynamicCPUData, DynamicLoadData, DynamicMonitoringData,
        DynamicMonitoringSummaryData, DynamicNetworkData, DynamicPerDiskData,
        DynamicPerNetworkInterfaceData, DynamicRamData, DynamicSystemData,
        StaticCPUData, StaticGpuData, StaticMonitoringData, StaticSystemData,
        is_excluded_file_system, is_excluded_mount, is_excluded_summary_disk,
        is_virtual_interface, scale_cpu_percent_to_i16, scale_load_to_i16,
    };

    fn disk(mount_point: &str, file_system: &str, total_space: u64) -> DynamicPerDiskData {
        DynamicPerDiskData {
            kind: DiskKind::Ssd,
            name: mount_point.to_owned(),
            file_system: file_system.to_owned(),
            mount_point: mount_point.to_owned(),
            total_space,
            available_space: total_space / 2,
            is_removable: false,
            is_read_only: false,
            read_speed: total_space / 10,
            write_speed: total_space / 20,
        }
    }

    fn monitoring_data(disks: Vec<DynamicPerDiskData>) -> DynamicMonitoringData {
        DynamicMonitoringData {
            uuid: uuid::Uuid::nil(),
            time: 0,
            cpu: DynamicCPUData {
                per_core: Vec::new(),
                total_cpu_usage: 0.0,
            },
            ram: DynamicRamData {
                total_memory: 0,
                available_memory: 0,
                used_memory: 0,
                total_swap: 0,
                used_swap: 0,
            },
            load: DynamicLoadData {
                one: 0.0,
                five: 0.0,
                fifteen: 0.0,
            },
            system: DynamicSystemData {
                boot_time: 0,
                uptime: 0,
                process_count: 0,
            },
            disk: disks,
            network: DynamicNetworkData {
                interfaces: Vec::new(),
                udp_connections: 0,
                tcp_connections: 0,
            },
            gpu: Vec::new(),
        }
    }

    fn disk_summary(disks: Vec<DynamicPerDiskData>) -> DynamicMonitoringSummaryData {
        DynamicMonitoringSummaryData::from_with_filter(&monitoring_data(disks), None, None)
    }

    #[test]
    fn cpu_percent_scales_normal_values() {
        assert_eq!(scale_cpu_percent_to_i16(53.4), Some(534));
        assert_eq!(scale_cpu_percent_to_i16(0.0), Some(0));
        assert_eq!(scale_cpu_percent_to_i16(100.0), Some(1000));
        assert_eq!(scale_cpu_percent_to_i16(99.95), Some(999));
    }

    #[test]
    fn cpu_percent_clamps_out_of_range() {
        assert_eq!(scale_cpu_percent_to_i16(150.0), Some(1000));
        assert_eq!(scale_cpu_percent_to_i16(1e9), Some(1000));
        assert_eq!(scale_cpu_percent_to_i16(-5.0), Some(0));
    }

    #[test]
    fn cpu_percent_nan_returns_none() {
        assert_eq!(scale_cpu_percent_to_i16(f64::NAN), None);
        assert_eq!(scale_cpu_percent_to_i16(f64::INFINITY), None);
        assert_eq!(scale_cpu_percent_to_i16(f64::NEG_INFINITY), None);
    }

    #[test]
    fn load_scales_normal_values() {
        assert_eq!(scale_load_to_i16(0.0), Some(0));
        assert_eq!(scale_load_to_i16(1.5), Some(15));
        assert_eq!(scale_load_to_i16(123.4), Some(1234));
    }

    #[test]
    fn load_clamps_to_i16_range() {
        assert_eq!(scale_load_to_i16(1e9), Some(i16::MAX));
        assert_eq!(scale_load_to_i16(-1e9), Some(i16::MIN));
    }

    #[test]
    fn load_nan_returns_none() {
        assert_eq!(scale_load_to_i16(f64::NAN), None);
        assert_eq!(scale_load_to_i16(f64::INFINITY), None);
    }

    #[test]
    fn excluded_mount_prefixes_match_path_boundaries() {
        assert!(is_excluded_mount("/run"));
        assert!(is_excluded_mount("/run/containerd"));
        assert!(is_excluded_mount("/tmp"));
        assert!(is_excluded_mount("/tmp/cache"));
        assert!(!is_excluded_mount("/runner"));
        assert!(!is_excluded_mount("/tmp-data"));
    }

    #[test]
    fn excluded_mount_prefixes_cover_kubernetes_volume_paths() {
        assert!(is_excluded_mount(
            "/var/lib/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount"
        ));
        assert!(is_excluded_mount(
            "/var/lib/rancher/k3s/agent/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount"
        ));
        assert!(is_excluded_mount(
            "/var/lib/kubelet/plugins/kubernetes.io/csi/driver.longhorn.io/volume-id/globalmount"
        ));
        assert!(is_excluded_mount(
            "/var/lib/rancher/k3s/agent/kubelet/plugins/kubernetes.io/csi/driver.longhorn.io/volume-id/globalmount"
        ));
    }

    #[test]
    fn excluded_file_systems_cover_pseudo_file_systems() {
        assert!(is_excluded_file_system("tmpfs"));
        assert!(is_excluded_file_system("overlay"));
        assert!(is_excluded_file_system("PROC"));
        assert!(!is_excluded_file_system("ext4"));
        assert!(!is_excluded_file_system("xfs"));
        assert!(!is_excluded_file_system("zfs"));
    }

    #[test]
    fn default_disk_summary_keeps_real_host_disks() {
        let summary = disk_summary(vec![disk("/", "ext4", 100), disk("/data", "xfs", 200)]);

        assert_eq!(summary.total_space, Some(300));
        assert_eq!(summary.available_space, Some(150));
        assert_eq!(summary.read_speed, Some(30));
        assert_eq!(summary.write_speed, Some(15));
    }

    #[test]
    fn default_disk_summary_excludes_kubernetes_volume_mounts() {
        let summary = disk_summary(vec![
            disk("/", "ext4", 100),
            disk(
                "/var/lib/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount",
                "ext4",
                50,
            ),
            disk(
                "/var/lib/rancher/k3s/agent/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount",
                "ext4",
                70,
            ),
            disk(
                "/var/lib/rancher/k3s/agent/kubelet/plugins/kubernetes.io/csi/driver.longhorn.io/volume-id/globalmount",
                "ext4",
                90,
            ),
        ]);

        assert_eq!(summary.total_space, Some(100));
        assert_eq!(summary.available_space, Some(50));
    }

    #[test]
    fn default_disk_summary_excludes_container_runtime_and_pseudo_disks() {
        let summary = disk_summary(vec![
            disk("/", "ext4", 100),
            disk(
                "/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/1/fs",
                "overlay",
                20,
            ),
            disk(
                "/var/lib/rancher/k3s/agent/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/1/fs",
                "overlay",
                20,
            ),
            disk("/run", "tmpfs", 10),
            disk("/proc", "proc", 10),
        ]);

        assert_eq!(summary.total_space, Some(100));
        assert_eq!(summary.available_space, Some(50));
    }

    #[test]
    fn default_disk_summary_keeps_longhorn_data_directory_mounts() {
        let summary = disk_summary(vec![
            disk("/", "ext4", 100),
            disk("/var/lib/longhorn", "xfs", 500),
        ]);

        assert_eq!(summary.total_space, Some(600));
        assert_eq!(summary.available_space, Some(300));
        assert!(!is_excluded_summary_disk(&disk(
            "/var/lib/longhorn",
            "xfs",
            500
        )));
    }

    #[test]
    fn explicit_disk_filter_keeps_existing_whitelist_behavior() {
        let data = monitoring_data(vec![
            disk("/", "ext4", 100),
            disk("/data", "xfs", 200),
            disk(
                "/var/lib/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount",
                "ext4",
                50,
            ),
        ]);

        let data_disk_filter = vec!["/data".to_owned()];
        let summary =
            DynamicMonitoringSummaryData::from_with_filter(&data, Some(&data_disk_filter), None);

        assert_eq!(summary.total_space, Some(200));
        assert_eq!(summary.available_space, Some(100));
    }

    #[test]
    fn explicit_disk_filter_can_select_default_excluded_mounts() {
        let kubelet_mount = "/var/lib/kubelet/pods/pod-id/volumes/kubernetes.io~csi/pvc-id/mount";
        let data = monitoring_data(vec![
            disk("/", "ext4", 100),
            disk(kubelet_mount, "ext4", 50),
        ]);
        let kubelet_disk_filter = vec![kubelet_mount.to_owned()];

        let summary =
            DynamicMonitoringSummaryData::from_with_filter(&data, Some(&kubelet_disk_filter), None);

        assert_eq!(summary.total_space, Some(50));
        assert_eq!(summary.available_space, Some(25));
    }

    // ── Additional tests ────────────────────────────────────────────────

    #[test]
    fn is_virtual_interface_common_prefixes() {
        assert!(is_virtual_interface("br0"));
        assert!(is_virtual_interface("cni0"));
        assert!(is_virtual_interface("docker0"));
        assert!(is_virtual_interface("podman0"));
        assert!(is_virtual_interface("lo"));
        assert!(is_virtual_interface("veth12345"));
        assert!(is_virtual_interface("virbr0"));
        assert!(is_virtual_interface("vmbr0"));
        assert!(is_virtual_interface("tap0"));
        assert!(is_virtual_interface("fwbr0"));
        assert!(is_virtual_interface("fwpr0"));
    }

    #[test]
    fn is_virtual_interface_physical_not_matched() {
        assert!(!is_virtual_interface("eth0"));
        assert!(!is_virtual_interface("ens192"));
        assert!(!is_virtual_interface("en0"));
        assert!(!is_virtual_interface("wlan0"));
    }

    #[test]
    fn is_excluded_mount_exact_and_subpath() {
        assert!(is_excluded_mount("/dev"));
        assert!(is_excluded_mount("/dev/sda1"));
        assert!(is_excluded_mount("/proc"));
        assert!(is_excluded_mount("/sys"));
        assert!(is_excluded_mount("/sys/fs/cgroup"));
        assert!(!is_excluded_mount("/dev-home"));
    }

    #[test]
    fn is_excluded_file_system_case_insensitive() {
        assert!(is_excluded_file_system("TMPFS"));
        assert!(is_excluded_file_system("Overlay"));
        assert!(is_excluded_file_system("cgroup2"));
        assert!(!is_excluded_file_system("btrfs"));
        assert!(!is_excluded_file_system("ext4"));
    }

    #[test]
    fn compute_data_hash_deterministic() {
        let cpu = StaticCPUData {
            physical_cores: 4,
            logical_cores: 8,
            per_core: vec![],
        };
        let system = StaticSystemData {
            system_name: "Linux".to_owned(),
            system_kernel: "5.15".to_owned(),
            system_kernel_version: "5.15.0".to_owned(),
            system_os_version: "22.04".to_owned(),
            system_os_long_version: "Ubuntu 22.04 LTS".to_owned(),
            distribution_id: "ubuntu".to_owned(),
            system_host_name: "test".to_owned(),
            arch: "x86_64".to_owned(),
            virtualization: "kvm".to_owned(),
        };
        let gpu: Vec<StaticGpuData> = vec![];

        let hash1 = StaticMonitoringData::compute_data_hash(&cpu, &system, &gpu).unwrap();
        let hash2 = StaticMonitoringData::compute_data_hash(&cpu, &system, &gpu).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
    }

    #[test]
    fn compute_data_hash_differs_on_content_change() {
        let cpu = StaticCPUData {
            physical_cores: 4,
            logical_cores: 8,
            per_core: vec![],
        };
        let system_a = StaticSystemData {
            system_name: "Linux".to_owned(),
            system_kernel: "5.15".to_owned(),
            system_kernel_version: "5.15.0".to_owned(),
            system_os_version: "22.04".to_owned(),
            system_os_long_version: "Ubuntu 22.04 LTS".to_owned(),
            distribution_id: "ubuntu".to_owned(),
            system_host_name: "host-a".to_owned(),
            arch: "x86_64".to_owned(),
            virtualization: "kvm".to_owned(),
        };
        let system_b = StaticSystemData {
            system_host_name: "host-b".to_owned(),
            ..system_a.clone()
        };

        let hash_a = StaticMonitoringData::compute_data_hash(&cpu, &system_a, &[]).unwrap();
        let hash_b = StaticMonitoringData::compute_data_hash(&cpu, &system_b, &[]).unwrap();
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn dynamic_monitoring_data_serde_roundtrip() {
        let data = monitoring_data(vec![]);
        let json = serde_json::to_string(&data).unwrap();
        let parsed: DynamicMonitoringData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.uuid, parsed.uuid);
        assert_eq!(data.time, parsed.time);
    }

    #[test]
    fn dynamic_monitoring_summary_data_serde_roundtrip() {
        let summary = DynamicMonitoringSummaryData {
            uuid: uuid::Uuid::nil(),
            time: 12345,
            cpu_usage: Some(534),
            gpu_usage: Some(80),
            used_swap: Some(0),
            total_swap: Some(0),
            used_memory: Some(8_589_934_592),
            total_memory: Some(17_179_869_184),
            available_memory: Some(8_589_934_592),
            load_one: Some(15),
            load_five: Some(10),
            load_fifteen: Some(5),
            uptime: Some(3600),
            boot_time: Some(1_700_000_000),
            process_count: Some(200),
            total_space: Some(500_000_000_000),
            available_space: Some(250_000_000_000),
            read_speed: Some(10_000_000),
            write_speed: Some(5_000_000),
            tcp_connections: Some(50),
            udp_connections: Some(10),
            total_received: Some(1_000_000_000),
            total_transmitted: Some(500_000_000),
            transmit_speed: Some(1_000_000),
            receive_speed: Some(2_000_000),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: DynamicMonitoringSummaryData = serde_json::from_str(&json).unwrap();
        assert_eq!(summary.uuid, parsed.uuid);
        assert_eq!(summary.cpu_usage, parsed.cpu_usage);
        assert_eq!(summary.used_memory, parsed.used_memory);
    }

    #[test]
    fn disk_kind_serde_roundtrip() {
        let variants = [
            (DiskKind::Hdd, "Hdd"),
            (DiskKind::Ssd, "Ssd"),
            (DiskKind::Unknown, "Unknown"),
        ];
        for (kind, name) in variants {
            let json = serde_json::to_string(&kind).unwrap();
            assert!(json.contains(name));
            let parsed: DiskKind = serde_json::from_str(&json).unwrap();
            let parsed_json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, parsed_json);
        }
    }

    #[test]
    fn from_trait_uses_default_exclusion() {
        let data = monitoring_data(vec![
            disk("/", "ext4", 100),
            disk("/run", "tmpfs", 50),
        ]);
        // From trait = from_with_filter(None, None)
        let summary_from = DynamicMonitoringSummaryData::from(&data);
        let summary_with_filter = DynamicMonitoringSummaryData::from_with_filter(&data, None, None);
        assert_eq!(summary_from.total_space, summary_with_filter.total_space);
        assert_eq!(summary_from.available_space, summary_with_filter.available_space);
    }

    #[test]
    fn network_interface_filter_selective() {
        let mut data = monitoring_data(vec![]);
        data.network = DynamicNetworkData {
            interfaces: vec![
                DynamicPerNetworkInterfaceData {
                    interface_name: "eth0".to_owned(),
                    total_received: 1000,
                    total_transmitted: 500,
                    receive_speed: 100,
                    transmit_speed: 50,
                },
                DynamicPerNetworkInterfaceData {
                    interface_name: "lo".to_owned(),
                    total_received: 200,
                    total_transmitted: 200,
                    receive_speed: 0,
                    transmit_speed: 0,
                },
            ],
            udp_connections: 0,
            tcp_connections: 0,
        };

        // Default filter should exclude "lo" (virtual interface)
        let summary = DynamicMonitoringSummaryData::from_with_filter(&data, None, None);
        assert_eq!(summary.total_received, Some(1000));
        assert_eq!(summary.total_transmitted, Some(500));

        // Explicit filter including lo
        let filter = vec!["lo".to_owned()];
        let summary = DynamicMonitoringSummaryData::from_with_filter(&data, None, Some(&filter));
        assert_eq!(summary.total_received, Some(200));
        assert_eq!(summary.total_transmitted, Some(200));

        // Empty filter falls back to default
        let empty: Vec<String> = vec![];
        let summary = DynamicMonitoringSummaryData::from_with_filter(&data, None, Some(&empty));
        assert_eq!(summary.total_received, Some(1000));
    }

    #[test]
    fn scale_cpu_percent_zero_and_max() {
        assert_eq!(scale_cpu_percent_to_i16(0.0), Some(0));
        assert_eq!(scale_cpu_percent_to_i16(100.0), Some(1000));
    }

    #[test]
    fn scale_load_zero_and_small() {
        assert_eq!(scale_load_to_i16(0.0), Some(0));
        assert_eq!(scale_load_to_i16(0.1), Some(1));
    }

    #[test]
    fn u64_to_i64_saturating_normal() {
        assert_eq!(super::u64_to_i64_saturating(0), 0);
        assert_eq!(super::u64_to_i64_saturating(100), 100);
    }

    #[test]
    fn u64_to_i64_saturating_overflow() {
        assert_eq!(super::u64_to_i64_saturating(u64::MAX), i64::MAX);
    }

    #[test]
    fn u64_to_i32_saturating_normal() {
        assert_eq!(super::u64_to_i32_saturating(0), 0);
        assert_eq!(super::u64_to_i32_saturating(100), 100);
    }

    #[test]
    fn u64_to_i32_saturating_overflow() {
        assert_eq!(super::u64_to_i32_saturating(u64::MAX), i32::MAX);
    }
}
