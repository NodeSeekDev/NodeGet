//! 监控数据查询字段枚举
//!
//! `StaticDataQueryField` 与 `DynamicDataQueryField` 分别对应
//! 静态采集（5 分钟级）和动态采集（秒级）数据的可查询维度。
//! 每个变体提供数据库列名（`column_name`）与 JSON 键名（`json_key`）两种映射。

use serde::{Deserialize, Serialize};

/// 静态监控数据可查询字段
#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StaticDataQueryField {
    /// CPU 静态信息（型号、核心数等）
    Cpu,
    /// 系统静态信息（主机名、OS 等）
    System,
    /// GPU 静态信息（型号、显存等）
    Gpu,
}

impl StaticDataQueryField {
    /// 返回数据库中对应的列名。
    #[must_use]
    pub const fn column_name(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu_data",
            Self::System => "system_data",
            Self::Gpu => "gpu_data",
        }
    }

    /// 返回 JSON 输出中使用的键名。
    #[must_use]
    pub const fn json_key(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::System => "system",
            Self::Gpu => "gpu",
        }
    }
}

/// 动态监控数据可查询字段
#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DynamicDataQueryField {
    /// CPU 使用率
    Cpu,
    /// 内存使用情况
    Ram,
    /// 系统负载
    Load,
    /// 系统级动态信息（运行时间等）
    System,
    /// 磁盘使用与 IO
    Disk,
    /// 网络流量与连接
    Network,
    /// GPU 使用率与显存
    Gpu,
}

impl DynamicDataQueryField {
    /// 返回数据库中对应的列名。
    #[must_use]
    pub const fn column_name(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu_data",
            Self::Ram => "ram_data",
            Self::Load => "load_data",
            Self::System => "system_data",
            Self::Disk => "disk_data",
            Self::Network => "network_data",
            Self::Gpu => "gpu_data",
        }
    }

    /// 返回 JSON 输出中使用的键名。
    #[must_use]
    pub const fn json_key(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Ram => "ram",
            Self::Load => "load",
            Self::System => "system",
            Self::Disk => "disk",
            Self::Network => "network",
            Self::Gpu => "gpu",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DynamicDataQueryField, StaticDataQueryField};

    // ── StaticDataQueryField ────────────────────────────────────────

    #[test]
    fn static_field_cpu() {
        assert_eq!(StaticDataQueryField::Cpu.column_name(), "cpu_data");
        assert_eq!(StaticDataQueryField::Cpu.json_key(), "cpu");
    }

    #[test]
    fn static_field_system() {
        assert_eq!(StaticDataQueryField::System.column_name(), "system_data");
        assert_eq!(StaticDataQueryField::System.json_key(), "system");
    }

    #[test]
    fn static_field_gpu() {
        assert_eq!(StaticDataQueryField::Gpu.column_name(), "gpu_data");
        assert_eq!(StaticDataQueryField::Gpu.json_key(), "gpu");
    }

    #[test]
    fn static_field_eq() {
        assert_eq!(StaticDataQueryField::Cpu, StaticDataQueryField::Cpu);
        assert_ne!(StaticDataQueryField::Cpu, StaticDataQueryField::Gpu);
    }

    #[test]
    fn static_field_clone_copy() {
        let a = StaticDataQueryField::Cpu;
        let b = a; // Copy
        let c = a; // Copy again
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn static_field_serde_round_trip() {
        for field in [StaticDataQueryField::Cpu, StaticDataQueryField::System, StaticDataQueryField::Gpu] {
            let json = serde_json::to_string(&field).unwrap();
            let de: StaticDataQueryField = serde_json::from_str(&json).unwrap();
            assert_eq!(field, de);
        }
    }

    #[test]
    fn static_field_debug() {
        let d = format!("{:?}", StaticDataQueryField::Cpu);
        assert_eq!(d, "Cpu");
    }

    // ── DynamicDataQueryField ───────────────────────────────────────

    #[test]
    fn dynamic_field_cpu() {
        assert_eq!(DynamicDataQueryField::Cpu.column_name(), "cpu_data");
        assert_eq!(DynamicDataQueryField::Cpu.json_key(), "cpu");
    }

    #[test]
    fn dynamic_field_ram() {
        assert_eq!(DynamicDataQueryField::Ram.column_name(), "ram_data");
        assert_eq!(DynamicDataQueryField::Ram.json_key(), "ram");
    }

    #[test]
    fn dynamic_field_load() {
        assert_eq!(DynamicDataQueryField::Load.column_name(), "load_data");
        assert_eq!(DynamicDataQueryField::Load.json_key(), "load");
    }

    #[test]
    fn dynamic_field_system() {
        assert_eq!(DynamicDataQueryField::System.column_name(), "system_data");
        assert_eq!(DynamicDataQueryField::System.json_key(), "system");
    }

    #[test]
    fn dynamic_field_disk() {
        assert_eq!(DynamicDataQueryField::Disk.column_name(), "disk_data");
        assert_eq!(DynamicDataQueryField::Disk.json_key(), "disk");
    }

    #[test]
    fn dynamic_field_network() {
        assert_eq!(DynamicDataQueryField::Network.column_name(), "network_data");
        assert_eq!(DynamicDataQueryField::Network.json_key(), "network");
    }

    #[test]
    fn dynamic_field_gpu() {
        assert_eq!(DynamicDataQueryField::Gpu.column_name(), "gpu_data");
        assert_eq!(DynamicDataQueryField::Gpu.json_key(), "gpu");
    }

    #[test]
    fn dynamic_field_eq() {
        assert_eq!(DynamicDataQueryField::Ram, DynamicDataQueryField::Ram);
        assert_ne!(DynamicDataQueryField::Ram, DynamicDataQueryField::Cpu);
    }

    #[test]
    fn dynamic_field_clone_copy() {
        let a = DynamicDataQueryField::Disk;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn dynamic_field_serde_round_trip() {
        for field in [
            DynamicDataQueryField::Cpu,
            DynamicDataQueryField::Ram,
            DynamicDataQueryField::Load,
            DynamicDataQueryField::System,
            DynamicDataQueryField::Disk,
            DynamicDataQueryField::Network,
            DynamicDataQueryField::Gpu,
        ] {
            let json = serde_json::to_string(&field).unwrap();
            let de: DynamicDataQueryField = serde_json::from_str(&json).unwrap();
            assert_eq!(field, de);
        }
    }

    #[test]
    fn dynamic_field_debug() {
        let d = format!("{:?}", DynamicDataQueryField::Ram);
        assert_eq!(d, "Ram");
    }
}
