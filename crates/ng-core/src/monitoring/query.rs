use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StaticDataQueryField {
    Cpu,
    System,
    Gpu,
}

impl StaticDataQueryField {
    #[must_use]
    pub const fn column_name(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu_data",
            Self::System => "system_data",
            Self::Gpu => "gpu_data",
        }
    }

    #[must_use]
    pub const fn json_key(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::System => "system",
            Self::Gpu => "gpu",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DynamicDataQueryField {
    Cpu,
    Ram,
    Load,
    System,
    Disk,
    Network,
    Gpu,
}

impl DynamicDataQueryField {
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
