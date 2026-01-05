use crate::entities::{
    dynamic_cpu_per_core, dynamic_disk, dynamic_monitoring, dynamic_network_interface,
    static_cpu_per_core, static_monitoring,
};
use crate::monitoring::data_structure::{
    DiskKind, DynamicCPUData, DynamicLoadData, DynamicMonitoringData,
    DynamicMonitoringDataForDatabase, DynamicNetworkData, DynamicPerCpuCoreData,
    DynamicPerDiskData, DynamicPerNetworkInterfaceData, DynamicRamData, DynamicSystemData,
    StaticCPUData, StaticMonitoringData, StaticMonitoringDataForDatabase, StaticPerCpuCoreData,
    StaticSystemData,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, LoaderTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use std::collections::HashSet;
use uuid::Uuid;

// =============================================================================
//  通用结构体与枚举
// =============================================================================

#[derive(Debug)]
pub enum MonitoringDatabaseError {
    Db(DbErr),
    NotFound(String),
}

impl From<DbErr> for MonitoringDatabaseError {
    fn from(err: DbErr) -> Self {
        MonitoringDatabaseError::Db(err)
    }
}

/// 通用查询过滤器
/// 所有字段均为 Option，由使用者组合条件
#[derive(Debug, Default, Clone)]
pub struct MonitoringQueryFilter {
    pub ids: Option<Vec<u64>>,
    pub node_uuids: Option<Vec<Uuid>>,
    pub start_time: Option<u128>,
    pub end_time: Option<u128>,
    pub limit: Option<u64>,
}

impl MonitoringQueryFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn id(mut self, id: u64) -> Self {
        self.ids = Some(vec![id]);
        self
    }

    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.node_uuids = Some(vec![uuid]);
        self
    }

    pub fn time_range(mut self, start: u128, end: u128) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// 静态监控数据字段选择器
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaticDataSelector {
    Cpu,    // 包含 cpu 核心表
    System, // 包含 system 相关字段
}

/// 动态监控数据字段选择器
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicDataSelector {
    Cpu,
    Ram,
    Load,
    System,
    Disk,
    Network,
}

// 全选
impl StaticDataSelector {
    pub fn all() -> HashSet<Self> {
        HashSet::from([Self::Cpu, Self::System])
    }
}

impl DynamicDataSelector {
    pub fn all() -> HashSet<Self> {
        HashSet::from([
            Self::Cpu,
            Self::Ram,
            Self::Load,
            Self::System,
            Self::Disk,
            Self::Network,
        ])
    }
}

// =============================================================================
//  Static Monitoring Operations (Unified)
// =============================================================================

/// 通用静态数据读取函数
pub async fn read_static_monitoring_data(
    db: &DatabaseConnection,
    filter: MonitoringQueryFilter,
    selectors: &HashSet<StaticDataSelector>,
) -> Result<Vec<StaticMonitoringDataForDatabase>, MonitoringDatabaseError> {
    // 1. 构建主表查询
    let mut query = static_monitoring::Entity::find();

    if let Some(ids) = filter.ids {
        let ids_i64: Vec<i64> = ids.into_iter().map(|id| id as i64).collect();
        query = query.filter(static_monitoring::Column::Id.is_in(ids_i64));
    }

    if let Some(uuids) = filter.node_uuids {
        query = query.filter(static_monitoring::Column::NodeUuid.is_in(uuids));
    }

    if let Some(start) = filter.start_time {
        query = query.filter(static_monitoring::Column::Time.gte(start as i64));
    }
    if let Some(end) = filter.end_time {
        query = query.filter(static_monitoring::Column::Time.lte(end as i64));
    }

    // 默认按时间倒序
    query = query.order_by_desc(static_monitoring::Column::Time);

    if let Some(limit) = filter.limit {
        query = query.limit(limit);
    }

    // 2. 获取主表数据
    let parents = query.all(db).await?;

    if parents.is_empty() {
        return Ok(vec![]);
    }

    // 3. 按需加载子表 (Loader Pattern)
    // 如果没有选择 CPU，则不查询 cpu_per_core 表，节省性能
    let cpu_cores_map = if selectors.contains(&StaticDataSelector::Cpu) {
        parents.load_many(static_cpu_per_core::Entity, db).await?
    } else {
        vec![vec![]; parents.len()]
    };

    // 4. 组装数据
    let mut result = Vec::with_capacity(parents.len());

    for (i, parent) in parents.into_iter().enumerate() {
        // 如果未选择 System，将相关字段置为空值或默认值
        let include_system = selectors.contains(&StaticDataSelector::System);

        let data = StaticMonitoringDataForDatabase {
            id: parent.id as u64,
            uuid: parent.node_uuid,
            time: parent.time as u128,
            data: StaticMonitoringData {
                cpu: StaticCPUData {
                    physical_cores: parent.physical_cores.unwrap_or(0) as u64,
                    logical_cores: parent.logical_cores.unwrap_or(0) as u64,
                    per_core: cpu_cores_map[i]
                        .iter()
                        .map(|c| StaticPerCpuCoreData {
                            id: c.core_id as u32,
                            name: c.core_name.clone(),
                            vendor_id: c.vendor_id.clone().unwrap_or_default(),
                            brand: c.brand.clone().unwrap_or_default(),
                        })
                        .collect(),
                },
                system: StaticSystemData {
                    system_name: if include_system {
                        parent.system_name.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    system_kernel: if include_system {
                        parent.system_kernel.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    system_kernel_version: if include_system {
                        parent.system_kernel_version.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    system_os_version: if include_system {
                        parent.system_os_version.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    system_os_long_version: if include_system {
                        parent.system_os_long_version.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    distribution_id: if include_system {
                        parent.distribution_id.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    system_host_name: if include_system {
                        parent.system_host_name.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    arch: if include_system {
                        parent.arch.unwrap_or_default()
                    } else {
                        String::new()
                    },
                    virtualization: if include_system {
                        parent.virtualization.unwrap_or_default()
                    } else {
                        String::new()
                    },
                },
            },
        };
        result.push(data);
    }

    Ok(result)
}

/// 通用静态数据写入函数
/// 仅写入 selectors 中指定的模块
pub async fn insert_static_monitoring_data(
    db: &DatabaseConnection,
    data: StaticMonitoringDataForDatabase,
    selectors: &HashSet<StaticDataSelector>,
) -> Result<(), MonitoringDatabaseError> {
    let txn = db.begin().await?;

    // 1. 准备主表数据
    let include_system = selectors.contains(&StaticDataSelector::System);
    let include_cpu = selectors.contains(&StaticDataSelector::Cpu); // 主表中也有 CPU 核心总数信息

    let static_model = static_monitoring::ActiveModel {
        node_uuid: Set(data.uuid),
        time: Set(data.time as i64),

        // CPU meta info in parent
        physical_cores: if include_cpu {
            Set(Some(data.data.cpu.physical_cores as i64))
        } else {
            Set(None)
        },
        logical_cores: if include_cpu {
            Set(Some(data.data.cpu.logical_cores as i64))
        } else {
            Set(None)
        },

        // System info
        system_name: if include_system {
            Set(Some(data.data.system.system_name))
        } else {
            Set(None)
        },
        system_kernel: if include_system {
            Set(Some(data.data.system.system_kernel))
        } else {
            Set(None)
        },
        system_kernel_version: if include_system {
            Set(Some(data.data.system.system_kernel_version))
        } else {
            Set(None)
        },
        system_os_version: if include_system {
            Set(Some(data.data.system.system_os_version))
        } else {
            Set(None)
        },
        system_os_long_version: if include_system {
            Set(Some(data.data.system.system_os_long_version))
        } else {
            Set(None)
        },
        distribution_id: if include_system {
            Set(Some(data.data.system.distribution_id))
        } else {
            Set(None)
        },
        system_host_name: if include_system {
            Set(Some(data.data.system.system_host_name))
        } else {
            Set(None)
        },
        arch: if include_system {
            Set(Some(data.data.system.arch))
        } else {
            Set(None)
        },
        virtualization: if include_system {
            Set(Some(data.data.system.virtualization))
        } else {
            Set(None)
        },
        ..Default::default()
    };

    let inserted = static_model.insert(&txn).await?;

    // 2. 写入关联表 (CPU Cores)
    if include_cpu && !data.data.cpu.per_core.is_empty() {
        let cpu_cores: Vec<static_cpu_per_core::ActiveModel> = data
            .data
            .cpu
            .per_core
            .into_iter()
            .map(|core| static_cpu_per_core::ActiveModel {
                static_monitoring_id: Set(inserted.id),
                core_id: Set(core.id as i64),
                core_name: Set(core.name),
                vendor_id: Set(Some(core.vendor_id)),
                brand: Set(Some(core.brand)),
                ..Default::default()
            })
            .collect();

        if !cpu_cores.is_empty() {
            static_cpu_per_core::Entity::insert_many(cpu_cores)
                .exec(&txn)
                .await?;
        }
    }

    txn.commit().await?;
    Ok(())
}

// =============================================================================
//  Dynamic Monitoring Operations (Unified)
// =============================================================================

/// 通用动态数据读取函数
pub async fn read_dynamic_monitoring_data(
    db: &DatabaseConnection,
    filter: MonitoringQueryFilter,
    selectors: &HashSet<DynamicDataSelector>,
) -> Result<Vec<DynamicMonitoringDataForDatabase>, MonitoringDatabaseError> {
    // 1. 构建主表查询
    let mut query = dynamic_monitoring::Entity::find();

    if let Some(ids) = filter.ids {
        let ids_i64: Vec<i64> = ids.into_iter().map(|id| id as i64).collect();
        query = query.filter(dynamic_monitoring::Column::Id.is_in(ids_i64));
    }
    if let Some(uuids) = filter.node_uuids {
        query = query.filter(dynamic_monitoring::Column::NodeUuid.is_in(uuids));
    }
    if let Some(start) = filter.start_time {
        query = query.filter(dynamic_monitoring::Column::Time.gte(start as i64));
    }
    if let Some(end) = filter.end_time {
        query = query.filter(dynamic_monitoring::Column::Time.lte(end as i64));
    }

    query = query.order_by_desc(dynamic_monitoring::Column::Time);

    if let Some(limit) = filter.limit {
        query = query.limit(limit);
    }

    let parents = query.all(db).await?;
    if parents.is_empty() {
        return Ok(vec![]);
    }

    let cpu_cores_loader = if selectors.contains(&DynamicDataSelector::Cpu) {
        parents.load_many(dynamic_cpu_per_core::Entity, db).await?
    } else {
        vec![vec![]; parents.len()]
    };

    let disks_loader = if selectors.contains(&DynamicDataSelector::Disk) {
        parents.load_many(dynamic_disk::Entity, db).await?
    } else {
        vec![vec![]; parents.len()]
    };

    let network_loader = if selectors.contains(&DynamicDataSelector::Network) {
        parents
            .load_many(dynamic_network_interface::Entity, db)
            .await?
    } else {
        vec![vec![]; parents.len()]
    };

    let mut result_list = Vec::with_capacity(parents.len());

    for (i, parent) in parents.into_iter().enumerate() {
        result_list.push(DynamicMonitoringDataForDatabase {
            id: parent.id as u64,
            uuid: parent.node_uuid,
            time: parent.time as u128,
            data: DynamicMonitoringData {
                cpu: DynamicCPUData {
                    total_cpu_usage: if selectors.contains(&DynamicDataSelector::Cpu) {
                        parent.total_cpu_usage.unwrap_or(0.0)
                    } else {
                        0.0
                    },
                    per_core: cpu_cores_loader[i]
                        .iter()
                        .map(|c| DynamicPerCpuCoreData {
                            id: c.core_id as u32,
                            cpu_usage: c.cpu_usage,
                            frequency_mhz: c.frequency_mhz.unwrap_or(0) as u64,
                        })
                        .collect(),
                },
                ram: DynamicRamData {
                    total_memory: if selectors.contains(&DynamicDataSelector::Ram) {
                        parent.total_memory.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    available_memory: if selectors.contains(&DynamicDataSelector::Ram) {
                        parent.available_memory.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    used_memory: if selectors.contains(&DynamicDataSelector::Ram) {
                        parent.used_memory.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    total_swap: if selectors.contains(&DynamicDataSelector::Ram) {
                        parent.total_swap.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    used_swap: if selectors.contains(&DynamicDataSelector::Ram) {
                        parent.used_swap.unwrap_or(0) as u64
                    } else {
                        0
                    },
                },
                load: DynamicLoadData {
                    one: if selectors.contains(&DynamicDataSelector::Load) {
                        parent.load_one.unwrap_or(0.0)
                    } else {
                        0.0
                    },
                    five: if selectors.contains(&DynamicDataSelector::Load) {
                        parent.load_five.unwrap_or(0.0)
                    } else {
                        0.0
                    },
                    fifteen: if selectors.contains(&DynamicDataSelector::Load) {
                        parent.load_fifteen.unwrap_or(0.0)
                    } else {
                        0.0
                    },
                },
                system: DynamicSystemData {
                    boot_time: if selectors.contains(&DynamicDataSelector::System) {
                        parent.boot_time.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    uptime: if selectors.contains(&DynamicDataSelector::System) {
                        parent.uptime.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    process_count: if selectors.contains(&DynamicDataSelector::System) {
                        parent.process_count.unwrap_or(0) as u64
                    } else {
                        0
                    },
                },
                disk: disks_loader[i]
                    .iter()
                    .map(|d| DynamicPerDiskData {
                        kind: match d.kind.as_str() {
                            "HDD" => DiskKind::Hdd,
                            "SSD" => DiskKind::Ssd,
                            _ => DiskKind::Unknown,
                        },
                        name: d.name.clone(),
                        file_system: d.file_system.clone().unwrap_or_default(),
                        mount_point: d.mount_point.clone(),
                        total_space: d.total_space.unwrap_or(0) as u64,
                        available_space: d.available_space.unwrap_or(0) as u64,
                        is_removable: d.is_removable,
                        is_read_only: d.is_read_only,
                        read_speed: d.read_speed.unwrap_or(0) as u64,
                        write_speed: d.write_speed.unwrap_or(0) as u64,
                    })
                    .collect(),
                network: DynamicNetworkData {
                    udp_connections: if selectors.contains(&DynamicDataSelector::Network) {
                        parent.udp_connections.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    tcp_connections: if selectors.contains(&DynamicDataSelector::Network) {
                        parent.tcp_connections.unwrap_or(0) as u64
                    } else {
                        0
                    },
                    interfaces: network_loader[i]
                        .iter()
                        .map(|n| DynamicPerNetworkInterfaceData {
                            interface_name: n.interface_name.clone(),
                            total_received: n.total_received.unwrap_or(0) as u64,
                            total_transmitted: n.total_transmitted.unwrap_or(0) as u64,
                            receive_speed: n.receive_speed.unwrap_or(0) as u64,
                            transmit_speed: n.transmit_speed.unwrap_or(0) as u64,
                        })
                        .collect(),
                },
            },
        });
    }

    Ok(result_list)
}

/// 通用动态数据写入函数
/// 仅写入 selectors 指定的模块
pub async fn insert_dynamic_monitoring_data(
    db: &DatabaseConnection,
    data: DynamicMonitoringDataForDatabase,
    selectors: &HashSet<DynamicDataSelector>,
) -> Result<(), MonitoringDatabaseError> {
    let txn = db.begin().await?;

    let use_cpu = selectors.contains(&DynamicDataSelector::Cpu);
    let use_ram = selectors.contains(&DynamicDataSelector::Ram);
    let use_load = selectors.contains(&DynamicDataSelector::Load);
    let use_system = selectors.contains(&DynamicDataSelector::System);
    let use_network = selectors.contains(&DynamicDataSelector::Network);
    let use_disk = selectors.contains(&DynamicDataSelector::Disk);

    // 1. Insert Parent
    let dynamic_model = dynamic_monitoring::ActiveModel {
        node_uuid: Set(data.uuid),
        time: Set(data.time as i64),

        total_cpu_usage: if use_cpu {
            Set(Some(data.data.cpu.total_cpu_usage))
        } else {
            Set(None)
        },

        total_memory: if use_ram {
            Set(Some(data.data.ram.total_memory as i64))
        } else {
            Set(None)
        },
        available_memory: if use_ram {
            Set(Some(data.data.ram.available_memory as i64))
        } else {
            Set(None)
        },
        used_memory: if use_ram {
            Set(Some(data.data.ram.used_memory as i64))
        } else {
            Set(None)
        },
        total_swap: if use_ram {
            Set(Some(data.data.ram.total_swap as i64))
        } else {
            Set(None)
        },
        used_swap: if use_ram {
            Set(Some(data.data.ram.used_swap as i64))
        } else {
            Set(None)
        },

        load_one: if use_load {
            Set(Some(data.data.load.one))
        } else {
            Set(None)
        },
        load_five: if use_load {
            Set(Some(data.data.load.five))
        } else {
            Set(None)
        },
        load_fifteen: if use_load {
            Set(Some(data.data.load.fifteen))
        } else {
            Set(None)
        },

        boot_time: if use_system {
            Set(Some(data.data.system.boot_time as i64))
        } else {
            Set(None)
        },
        uptime: if use_system {
            Set(Some(data.data.system.uptime as i64))
        } else {
            Set(None)
        },
        process_count: if use_system {
            Set(Some(data.data.system.process_count as i64))
        } else {
            Set(None)
        },

        udp_connections: if use_network {
            Set(Some(data.data.network.udp_connections as i64))
        } else {
            Set(None)
        },
        tcp_connections: if use_network {
            Set(Some(data.data.network.tcp_connections as i64))
        } else {
            Set(None)
        },

        ..Default::default()
    };

    let inserted = dynamic_model.insert(&txn).await?;
    let pid = inserted.id;

    // 2. Insert Children (only if selected AND data is present)

    if use_cpu && !data.data.cpu.per_core.is_empty() {
        let rows: Vec<dynamic_cpu_per_core::ActiveModel> = data
            .data
            .cpu
            .per_core
            .into_iter()
            .map(|c| dynamic_cpu_per_core::ActiveModel {
                dynamic_monitoring_id: Set(pid),
                core_id: Set(c.id as i64),
                cpu_usage: Set(c.cpu_usage),
                frequency_mhz: Set(Some(c.frequency_mhz as i64)),
                ..Default::default()
            })
            .collect();
        if !rows.is_empty() {
            dynamic_cpu_per_core::Entity::insert_many(rows)
                .exec(&txn)
                .await?;
        }
    }

    if use_disk && !data.data.disk.is_empty() {
        let rows: Vec<dynamic_disk::ActiveModel> = data
            .data
            .disk
            .into_iter()
            .map(|d| dynamic_disk::ActiveModel {
                dynamic_monitoring_id: Set(pid),
                kind: Set(match d.kind {
                    DiskKind::Hdd => "HDD".to_string(),
                    DiskKind::Ssd => "SSD".to_string(),
                    DiskKind::Unknown => "Unknown".to_string(),
                }),
                name: Set(d.name),
                file_system: Set(Some(d.file_system)),
                mount_point: Set(d.mount_point),
                total_space: Set(Some(d.total_space as i64)),
                available_space: Set(Some(d.available_space as i64)),
                is_removable: Set(d.is_removable),
                is_read_only: Set(d.is_read_only),
                read_speed: Set(Some(d.read_speed as i64)),
                write_speed: Set(Some(d.write_speed as i64)),
                ..Default::default()
            })
            .collect();
        if !rows.is_empty() {
            dynamic_disk::Entity::insert_many(rows).exec(&txn).await?;
        }
    }

    if use_network && !data.data.network.interfaces.is_empty() {
        let rows: Vec<dynamic_network_interface::ActiveModel> = data
            .data
            .network
            .interfaces
            .into_iter()
            .map(|n| dynamic_network_interface::ActiveModel {
                dynamic_monitoring_id: Set(pid),
                interface_name: Set(n.interface_name),
                total_received: Set(Some(n.total_received as i64)),
                total_transmitted: Set(Some(n.total_transmitted as i64)),
                receive_speed: Set(Some(n.receive_speed as i64)),
                transmit_speed: Set(Some(n.transmit_speed as i64)),
                ..Default::default()
            })
            .collect();
        if !rows.is_empty() {
            dynamic_network_interface::Entity::insert_many(rows)
                .exec(&txn)
                .await?;
        }
    }

    txn.commit().await?;
    Ok(())
}
