// 若数据量字段中未注明单位，则以字节 (Bytes) 为单位

use sha2::{Digest, Sha256};

// 静态监控数据结构体，包含不会随时间变化的硬件信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticMonitoringData {
    // 设备 UUID
    pub uuid: uuid::Uuid,
    // 时间戳（毫秒）
    pub time: u64,
    // 数据内容的 SHA-256 哈希（前 16 字节原始二进制），用于去重
    pub data_hash: Vec<u8>,

    // CPU 静态信息
    pub cpu: StaticCPUData,
    // 系统静态信
    pub system: StaticSystemData,
    // GPU 静态信息列表
    pub gpu: Vec<StaticGpuData>,
}

/// 将 u64 安全饱和转换为 i64，超过 `i64::MAX` 时返回 `i64::MAX，避免静默回绕`
#[must_use]
fn u64_to_i64_saturating(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// 将 u64 安全饱和转换为 i32，超过 `i32::MAX` 时返回 `i32::MAX，避免静默回绕`
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

// 将 `std::io::Write` 调用桥接到 `Sha256::update`，实现零分配流式哈希
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

// 动态监控数据结构体，包含随时间变化的系统状态信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringData {
    // 设备 UUID
    pub uuid: uuid::Uuid,
    // 时间戳（毫秒）
    pub time: u64,

    // CPU 动态信息
    pub cpu: DynamicCPUData,
    // 内存动态信息
    pub ram: DynamicRamData,
    // 系统负载动态信息
    pub load: DynamicLoadData,
    // 系统动态信息
    pub system: DynamicSystemData,
    // 磁盘动态信息列表
    pub disk: Vec<DynamicPerDiskData>,
    // 网络动态信息
    pub network: DynamicNetworkData,
    // GPU 动态信息列表
    pub gpu: Vec<DynamicGpuData>,
}

// 动态监控摘要数据结构体，包含扁平化的系统状态摘要信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicMonitoringSummaryData {
    // 设备 UUID
    pub uuid: uuid::Uuid,
    // 时间戳（毫秒）
    pub time: u64,

    pub cpu_usage: Option<i16>,
    pub gpu_usage: Option<i16>,
    pub used_swap: Option<i64>,
    pub total_swap: Option<i64>,
    pub used_memory: Option<i64>,
    pub total_memory: Option<i64>,
    pub available_memory: Option<i64>,
    pub load_one: Option<i16>,
    pub load_five: Option<i16>,
    pub load_fifteen: Option<i16>,
    pub uptime: Option<i32>,
    pub boot_time: Option<i64>,
    pub process_count: Option<i32>,
    pub total_space: Option<i64>,
    pub available_space: Option<i64>,
    pub read_speed: Option<i64>,
    pub write_speed: Option<i64>,
    pub tcp_connections: Option<i32>,
    pub udp_connections: Option<i32>,
    pub total_received: Option<i64>,
    pub total_transmitted: Option<i64>,
    pub transmit_speed: Option<i64>,
    pub receive_speed: Option<i64>,
}

const VIRTUAL_INTERFACE_PREFIXES: &[&str] = &[
    "br", "cni", "docker", "podman", "flannel", "lo", "veth", "virbr", "vmbr", "tap", "fwbr",
    "fwpr",
];

const EXCLUDED_MOUNT_PREFIXES: &[&str] = &[
    "/tmp",
    "/var/tmp",
    "/dev",
    "/run",
    "/var/lib/containers",
    "/var/lib/docker",
    "/proc",
    "/sys",
    "/sys/fs/cgroup",
    "/etc/resolv.conf",
    "/etc/host",
    "/nix/store",
];

#[must_use]
pub fn is_virtual_interface(name: &str) -> bool {
    VIRTUAL_INTERFACE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[must_use]
pub fn is_excluded_mount(mount_point: &str) -> bool {
    EXCLUDED_MOUNT_PREFIXES
        .iter()
        .any(|prefix| mount_point.starts_with(prefix))
}

/// Scale a percent value in the expected range `[0.0, 100.0]` by `10` and
/// encode it as the `i16` shape used by `dynamic_monitoring_summary.cpu_usage`.
///
/// The column semantically holds a percentage with one decimal place
/// (`value/10.0` on read). This helper is the single place that enforces
/// that invariant on the write side and guards against two classes of
/// upstream corruption that the previous `clamp(i16::MIN..=i16::MAX)` did
/// **not** catch:
///
/// * **`NaN` / `±Infinity`** — `f64::clamp` is a no-op on `NaN`, and
///   `f64 as i16` then silently folds the result to `0`, which would show
///   up as "0% CPU" on the dashboard. We return `None` instead so the
///   server records a gap rather than fabricated zeroes.
/// * **Out-of-range percentages (e.g. sysinfo returning `> 100.0` on a
///   container first-sample edge case)** — the previous clamp allowed up
///   to `i16::MAX = 32767` (i.e. 3276.7%), which propagated straight into
///   the database. We now clamp to `[0, 1000]` so the summary is always in
///   the documented `[0.0, 100.0]` range post-descaling.
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

/// Scale a 1/5/15-minute load average by `10` and encode as `i16`, matching
/// the `load_one` / `load_five` / `load_fifteen` column shape.
///
/// Unlike CPU percent, load averages can legitimately exceed 100 on heavily
/// contended systems (e.g. load of 200 on a 256-thread machine). We still
/// clamp to `i16` range to avoid silent `as i16` wrap-around, but the upper
/// bound is `i16::MAX` rather than `1000`. `NaN` is again represented as
/// `None` (missing datum) instead of being folded to `0`.
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
                .filter(|d| !is_excluded_mount(&d.mount_point))
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

// CPU 静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticCPUData {
    // 物理核心数
    pub physical_cores: u64,
    // 逻辑核心数
    pub logical_cores: u64,
    // 每个 CPU 核心的静态信息列表
    pub per_core: Vec<StaticPerCpuCoreData>,
}

// CPU 动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicCPUData {
    // 每个 CPU 核心的动态信息列表
    pub per_core: Vec<DynamicPerCpuCoreData>,
    // CPU 总使用率（0-100）
    pub total_cpu_usage: f64,
}

// 每个 CPU 核心的静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticPerCpuCoreData {
    // 核心 ID，从 1 开始
    pub id: u32,
    // 核心名称
    pub name: String,
    // 供应商 ID
    pub vendor_id: String,
    // CPU 品牌
    pub brand: String,
}

// 每个 CPU 核心的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerCpuCoreData {
    // 核心 ID，从 1 开始
    pub id: u32,
    // CPU 使用率（0-100）
    pub cpu_usage: f64,
    // CPU 频率（MHz）
    pub frequency_mhz: u64,
}

// 内存动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicRamData {
    // 总内存大小（字节）
    pub total_memory: u64,
    // 可用内存大小（字节）
    pub available_memory: u64,
    // 已使用内存大小（字节）
    pub used_memory: u64,
    // 总交换空间大小（字节）
    pub total_swap: u64,
    // 已使用交换空间大小（字节）
    pub used_swap: u64,
}

// 系统负载动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicLoadData {
    // 1分钟平均负载
    pub one: f64,
    // 5分钟平均负载
    pub five: f64,
    // 15分钟平均负载
    pub fifteen: f64,
}

// 系统静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticSystemData {
    // 系统名称
    pub system_name: String,
    // 系统内核版本
    pub system_kernel: String,
    // 系统内核详细版本
    pub system_kernel_version: String,
    // 系统操作系统版本
    pub system_os_version: String,
    // 系统操作系统详细版本
    pub system_os_long_version: String,
    // 发行版 ID
    pub distribution_id: String,
    // 系统主机名
    pub system_host_name: String,
    // 系统架构
    pub arch: String,
    // 虚拟化平台
    pub virtualization: String,
}

// 系统动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicSystemData {
    // 系统启动时间（秒时间戳）
    pub boot_time: u64,
    // 系统运行时间（秒）
    pub uptime: u64,
    // 进程数量
    pub process_count: u64,
}

// 磁盘类型枚举
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum DiskKind {
    // 机械硬盘
    Hdd,
    // 固态硬盘
    Ssd,
    // 未知类型
    Unknown,
}

// 每个磁盘的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerDiskData {
    // 磁盘类型
    pub kind: DiskKind,
    // 磁盘名称
    pub name: String,
    // 文件系统类型
    pub file_system: String,
    // 挂载点
    pub mount_point: String,
    // 总空间大小（字节）
    pub total_space: u64,
    // 可用空间大小（字节）
    pub available_space: u64,
    // 是否可移动
    pub is_removable: bool,
    // 是否只读
    pub is_read_only: bool,
    // 读取速度（字节/秒）
    pub read_speed: u64,
    // 写入速度（字节/秒）
    pub write_speed: u64,
}

// 网络动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicNetworkData {
    // 网络接口列表
    pub interfaces: Vec<DynamicPerNetworkInterfaceData>,
    // UDP 连接数
    pub udp_connections: u64,
    // TCP 连接数
    pub tcp_connections: u64,
}

// 每个网络接口的动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicPerNetworkInterfaceData {
    // 网络接口名称
    pub interface_name: String,
    // 总接收数据量（字节），从上次网卡重启开始计算
    pub total_received: u64, // 从上次网卡重启开始计算
    // 总发送数据量（字节），从上次网卡重启开始计算
    pub total_transmitted: u64, // 从上次网卡重启开始计算
    // 接收速度（字节/秒）
    pub receive_speed: u64,
    // 发送速度（字节/秒）
    pub transmit_speed: u64,
}

// GPU 静态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StaticGpuData {
    // GPU ID，从 1 开始
    pub id: u32,
    // GPU 名称
    pub name: String,
    // CUDA 核心数（对于非 NVIDIA 显卡，该值为 0）
    pub cuda_cores: u64, // 对于非 NVIDIA 显卡，该值为 0
    // GPU 架构
    pub architecture: String,
}

// GPU 动态信息结构体
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DynamicGpuData {
    // GPU ID，从 1 开始
    pub id: u32,
    // 已使用显存（字节）
    pub used_memory: u64,
    // 总显存（字节）
    pub total_memory: u64,
    // 图形时钟频率（MHz）
    pub graphics_clock_mhz: u64,
    // 流处理器时钟频率（MHz），NV: Streaming Multiprocessor; AMD: Compute Unit
    pub sm_clock_mhz: u64, // NV: Streaming Multiprocessor; AMD: Compute Unit
    // 显存时钟频率（MHz）
    pub memory_clock_mhz: u64,
    // 视频时钟频率（MHz）
    pub video_clock_mhz: u64,
    // GPU 使用率百分比
    pub utilization_gpu: u8,
    // 显存使用率百分比 (不是显存占用率，反应内存读写频率的数值)
    pub utilization_memory: u8,
    // 温度（摄氏度）
    pub temperature: u8,
}

#[cfg(test)]
mod tests {
    use super::{scale_cpu_percent_to_i16, scale_load_to_i16};

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
}
