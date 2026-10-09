//! 流量统计。
//!
//! 根据 Agent 上报的出口网卡累计值，维护每块网卡的总流量（跨重启累加），
//! 定时写入数据库；计数器重置前可能丢失数据时，记录可能丢失数据的时间段。
//! 总流量快照由 JS Worker 通过 `agent_write_traffic_snapshot` 写入，不在这里生成。
//! 由 `report_dynamic` 调用 `update_total_traffic`。

use crate::data_structure::DynamicMonitoringData;
use ng_db::entity::traffic_current_total::Column as CurrentTotal;
use ng_db::entity::{traffic_current_total, traffic_possible_data_loss};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveValue, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait, Iterable, Set,
    TransactionTrait,
};
// 只有 `traffic_tables_on_sqlite` 用到，它仅在测试里编译
#[cfg(test)]
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval, timeout};
use tracing::{debug, error, info, warn};

/// 判定可能丢失数据的最短间隔（毫秒），10 分钟
const POSSIBLE_DATA_LOSS_THRESHOLD_MS: i64 = 10 * 60 * 1000;
/// 定时写库间隔（毫秒）
const FLUSH_INTERVAL_MS: u64 = 60 * 1000;
/// `flush_and_shutdown` 等待最后一次写库的最长时间
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// `SQLite` 单条 SQL 最多绑定的参数数量
const SQLITE_MAX_VARIABLES: usize = 999;
/// `PostgreSQL` 单条 SQL 最多绑定的参数数量
const POSTGRES_MAX_VARIABLES: usize = 65_535;

/// 全局 `TrafficStats` 单例。
///
/// 用 `Mutex<Option<…>>` 而非 `OnceLock`：配置热重载时 `flush_and_shutdown` 取走旧实例，
/// 重新 `init` 时从（可能已更换的）数据库重建，并重启 `flush_loop`。
static TRAFFIC_STATS: Mutex<Option<Arc<TrafficStats>>> = Mutex::new(None);

/// `flush_loop` 任务，`flush_and_shutdown` 通过它等待最后一次写库完成。
static FLUSH_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// 单块网卡的一次读数
#[derive(Debug, Clone)]
struct NetworkInterfaceReading {
    /// 开机标识
    boot_id: Option<String>,
    /// 网卡编号
    ifindex: Option<u32>,
    /// 网卡计数器的累计接收量（字节）
    counter_received: u64,
    /// 网卡计数器的累计发送量（字节）
    counter_transmitted: u64,
    /// Agent 采集时间（毫秒时间戳）
    report_time: i64,
    /// 创建时间（毫秒时间戳）
    created_at: i64,
    /// 更新时间（毫秒时间戳）
    updated_at: i64,
}

/// 收发流量
#[derive(Debug, Clone, Copy, Default)]
struct Traffic {
    /// 接收量（字节）
    received: u64,
    /// 发送量（字节）
    transmitted: u64,
}

/// 一块网卡当前的总流量，供查询接口读取
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCurrentTotal {
    /// 设备编号
    pub uuid_id: i16,
    /// 网卡名
    pub interface_name: String,
    /// 总接收量（字节）
    pub total_received: u64,
    /// 总发送量（字节）
    pub total_transmitted: u64,
    /// 最近一次更新时间（毫秒时间戳）
    pub updated_at: i64,
}

/// 流量统计的内存状态
#[derive(Debug, Default)]
struct State {
    /// (设备编号, 网卡名) → 上一次的网卡读数
    prev_readings: HashMap<(i16, String), NetworkInterfaceReading>,
    /// (设备编号, 网卡名) → 总流量
    totals: HashMap<(i16, String), Traffic>,
    /// 尚未写库的可能丢失数据的时间段
    pending_possible_data_losses: Vec<traffic_possible_data_loss::ActiveModel>,
}

impl State {
    /// 列出所有网卡当前的总流量，顺序不固定。
    fn current_totals(&self) -> Vec<InterfaceCurrentTotal> {
        self.prev_readings
            .iter()
            .map(|(key, reading)| {
                let total = self.totals.get(key).copied().unwrap_or_default();
                InterfaceCurrentTotal {
                    uuid_id: key.0,
                    interface_name: key.1.clone(),
                    total_received: total.received,
                    total_transmitted: total.transmitted,
                    updated_at: reading.updated_at,
                }
            })
            .collect()
    }

    /// 用一条动态监控上报更新内存状态。
    ///
    /// - `uuid_id`: 设备编号
    /// - `data`: 动态监控数据
    /// - `received_at`: 服务端收到数据的时间（毫秒时间戳）
    ///
    /// 1. 有网卡缺少 `is_outlet`（老版本 Agent）时直接跳过
    /// 2. 逐块处理 `is_outlet` 为真的网卡：
    ///    1. 用本次上报组成本次读数，取出上一次的读数；Agent 采集时间早于上一次的直接丢弃
    ///    2. 按方向判断是否重置（`detect_counter_reset`），算出本次流量增量
    ///       （`compute_traffic_increase`），加到总流量
    ///    3. 发生重置时判断是否可能丢失数据（`detect_possible_data_loss`），同一台设备同一时间段只记一次
    ///    4. 用本次读数替换上一次的读数
    fn apply_report(&mut self, uuid_id: i16, data: &DynamicMonitoringData, received_at: i64) {
        let interfaces = &data.network.interfaces;
        if interfaces
            .iter()
            .any(|interface| interface.is_outlet.is_none())
        {
            return;
        }

        let report_time = data.time.cast_signed();
        // 开机时间用服务端收到时间推算，不受 VPS 时钟影响
        let boot_at = received_at - data.system.uptime.saturating_mul(1000).cast_signed();
        let mut possible_data_loss_recorded = false;

        for interface in interfaces
            .iter()
            .filter(|interface| interface.is_outlet == Some(true))
        {
            let key = (uuid_id, interface.interface_name.clone());
            let prev_reading = self.prev_readings.get(&key);
            if prev_reading.is_some_and(|prev| report_time < prev.report_time) {
                continue;
            }

            let current_reading = NetworkInterfaceReading {
                boot_id: data.system.boot_id.clone(),
                ifindex: interface.ifindex,
                counter_received: interface.total_received,
                counter_transmitted: interface.total_transmitted,
                report_time,
                created_at: prev_reading.map_or(received_at, |prev| prev.created_at),
                updated_at: received_at,
            };
            let total = self.totals.get(&key).copied().unwrap_or_default();

            let reset = prev_reading.map_or(CounterReset::NONE, |prev| {
                detect_counter_reset(prev, &current_reading)
            });
            let increase = compute_traffic_increase(prev_reading, &current_reading, reset);
            self.totals.insert(
                key.clone(),
                Traffic {
                    received: total.received.saturating_add(increase.received),
                    transmitted: total.transmitted.saturating_add(increase.transmitted),
                },
            );

            if reset.any()
                && !possible_data_loss_recorded
                && let Some(prev) = prev_reading
            {
                let reset_at = if is_boot_id_changed(prev, &current_reading) {
                    boot_at
                } else {
                    received_at
                };
                if let Some(loss) = detect_possible_data_loss(uuid_id, prev, reset_at) {
                    self.pending_possible_data_losses.push(loss);
                    possible_data_loss_recorded = true;
                }
            }

            self.prev_readings.insert(key, current_reading);
        }
    }
}

/// 流量统计
pub struct TrafficStats {
    /// 内存状态
    state: Mutex<State>,
    /// 通知 `flush_loop` 做最后一次写库并退出
    shutdown: Notify,
}

impl TrafficStats {
    /// 初始化全局单例，并启动定时写库循环。
    ///
    /// 1. 从数据库读取每块网卡上一次的读数和总流量
    /// 2. 创建实例，放入 `TRAFFIC_STATS`（替换已有实例）
    /// 3. 启动 `flush_loop`，任务存入 `FLUSH_TASK`
    ///
    /// # Errors
    ///
    /// - 数据库连接未初始化或读取失败时返回错误，不创建实例，由调用方决定如何处理
    pub async fn init() -> anyhow::Result<()> {
        let db = ng_db::get_db().ok_or_else(|| anyhow::anyhow!("database not initialized"))?;
        let rows = traffic_current_total::Entity::find().all(db).await?;

        let mut state = State::default();
        for row in rows {
            let key = (row.uuid_id, row.interface_name);
            state.totals.insert(
                key.clone(),
                Traffic {
                    received: row.total_received.cast_unsigned(),
                    transmitted: row.total_transmitted.cast_unsigned(),
                },
            );
            state.prev_readings.insert(
                key,
                NetworkInterfaceReading {
                    boot_id: row.boot_id,
                    ifindex: row.ifindex.map(i32::cast_unsigned),
                    counter_received: row.counter_received.cast_unsigned(),
                    counter_transmitted: row.counter_transmitted.cast_unsigned(),
                    report_time: row.report_time,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                },
            );
        }
        let interface_count = state.prev_readings.len();

        let stats = Arc::new(Self {
            state: Mutex::new(state),
            shutdown: Notify::new(),
        });
        let replaced = TRAFFIC_STATS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(Arc::clone(&stats));
        if replaced.is_some() {
            warn!(target: "monitoring", "Traffic stats re-initialized without shutdown, previous instance replaced");
        }
        let task = tokio::spawn(flush_loop(stats));
        *FLUSH_TASK.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);

        info!(target: "monitoring", interfaces = interface_count, "Traffic stats initialized");
        Ok(())
    }

    /// 用一条动态监控上报更新总流量。
    ///
    /// - `uuid_id`: 设备编号（`MonitoringUuidCache` 分配）
    /// - `data`: 动态监控数据
    /// - `received_at`: 服务端收到数据的时间（毫秒时间戳）
    ///
    /// 1. 从 `TRAFFIC_STATS` 取出实例，未初始化或已关闭时直接返回
    /// 2. 更新内存状态（`State::apply_report`）
    pub fn update_total_traffic(uuid_id: i16, data: &DynamicMonitoringData, received_at: i64) {
        let Some(stats) = TRAFFIC_STATS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            return;
        };
        stats
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply_report(uuid_id, data, received_at);
    }

    /// 列出所有网卡当前的总流量。
    ///
    /// - 返回: 未初始化或已关闭时为 `None`；包含已软删除设备的网卡，由调用方过滤
    pub fn current_totals() -> Option<Vec<InterfaceCurrentTotal>> {
        let stats = TRAFFIC_STATS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let totals = stats
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .current_totals();
        Some(totals)
    }

    /// 把内存中的变化写入数据库。
    ///
    /// 1. 取出并清空尚未写库的可能丢失数据的时间段；复制所有网卡的上一次读数和总流量
    /// 2. 在同一个事务中写入（`write_to_db`）
    /// 3. 写库失败时把取出的可能丢失数据的时间段放回内存，下次重试
    async fn flush(&self) {
        let (possible_data_losses, current_totals) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let current_totals: Vec<_> = state
                .prev_readings
                .iter()
                .map(|(key, reading)| {
                    let total = state.totals.get(key).copied().unwrap_or_default();
                    current_total_model(key.0, &key.1, reading, total)
                })
                .collect();
            (
                std::mem::take(&mut state.pending_possible_data_losses),
                current_totals,
            )
        };
        if possible_data_losses.is_empty() && current_totals.is_empty() {
            return;
        }

        let result = match ng_db::get_db() {
            Some(db) => write_to_db(db, current_totals, possible_data_losses.clone()).await,
            None => Err(DbErr::Custom("database not initialized".to_owned())),
        };

        match result {
            Ok(()) => {
                debug!(target: "monitoring", "Traffic stats flushed");
            }
            Err(e) => {
                error!(target: "monitoring", error = %e, "Traffic stats flush failed, will retry next time");
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                state
                    .pending_possible_data_losses
                    .splice(0..0, possible_data_losses);
            }
        }
    }

    /// 写入剩余数据并关闭，服务端关闭或配置热重载时调用。
    ///
    /// 1. 从 `TRAFFIC_STATS` 取走实例，未初始化时直接返回
    /// 2. 通过 `shutdown` 通知 `flush_loop` 做最后一次写库并退出
    /// 3. 等待 `FLUSH_TASK` 结束，最多 5 秒
    pub async fn flush_and_shutdown() {
        let Some(stats) = TRAFFIC_STATS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        // notify_one 在 flush_loop 正在写库时会保留通知，写完后仍能收到
        stats.shutdown.notify_one();

        let task = FLUSH_TASK
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            match timeout(SHUTDOWN_TIMEOUT, task).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    warn!(target: "monitoring", error = %e, "Traffic stats flush loop panicked");
                }
                Err(_) => {
                    warn!(target: "monitoring", "Traffic stats flush loop did not exit within 5s timeout");
                }
            }
        }
        debug!(target: "monitoring", "Traffic stats shutdown complete");
    }
}

/// 定时写库循环。
///
/// - `stats`: 流量统计实例
///
/// 1. 每 `FLUSH_INTERVAL_MS` 调用一次 `flush`
/// 2. 收到 `shutdown` 通知时最后调用一次 `flush` 后退出
async fn flush_loop(stats: Arc<TrafficStats>) {
    let mut ticker = interval(Duration::from_millis(FLUSH_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // 第一次 tick 立即完成，跳过，避免启动时空写一次
    ticker.tick().await;

    loop {
        tokio::select! {
            () = stats.shutdown.notified() => {
                stats.flush().await;
                return;
            }
            _ = ticker.tick() => stats.flush().await,
        }
    }
}

/// 在同一个事务中写入当前总流量和可能丢失数据的时间段。
///
/// - `db`: 数据库连接
/// - `current_totals`: 每块网卡的上一次读数和总流量
/// - `possible_data_losses`: 待插入的可能丢失数据的时间段
/// - 返回: 任一步失败时返回错误，事务回滚
///
/// 按单条 SQL 的参数上限（`max_rows_per_statement`）分批写入：
/// 1. 写入当前总流量，同一 (设备, 网卡) 已存在则更新
/// 2. 插入可能丢失数据的时间段
async fn write_to_db(
    db: &DatabaseConnection,
    current_totals: Vec<traffic_current_total::ActiveModel>,
    possible_data_losses: Vec<traffic_possible_data_loss::ActiveModel>,
) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let txn = db.begin().await?;

    let on_conflict = OnConflict::columns([CurrentTotal::UuidId, CurrentTotal::InterfaceName])
        .update_columns([
            CurrentTotal::BootId,
            CurrentTotal::Ifindex,
            CurrentTotal::CounterReceived,
            CurrentTotal::CounterTransmitted,
            CurrentTotal::ReportTime,
            CurrentTotal::TotalReceived,
            CurrentTotal::TotalTransmitted,
            CurrentTotal::UpdatedAt,
        ])
        .to_owned();
    let rows = max_rows_per_statement(backend, traffic_current_total::Column::iter().count());
    for chunk in current_totals.chunks(rows) {
        traffic_current_total::Entity::insert_many(chunk.to_vec())
            .on_conflict(on_conflict.clone())
            .exec_without_returning(&txn)
            .await?;
    }

    let rows = max_rows_per_statement(backend, traffic_possible_data_loss::Column::iter().count());
    for chunk in possible_data_losses.chunks(rows) {
        traffic_possible_data_loss::Entity::insert_many(chunk.to_vec())
            .exec_without_returning(&txn)
            .await?;
    }

    txn.commit().await
}

/// 计算单条 INSERT 最多能写入的行数。
///
/// - `backend`: 数据库类型
/// - `columns`: 每行的列数
/// - 返回: 单条 SQL 的参数上限除以列数，至少为 1
pub(crate) fn max_rows_per_statement(backend: DatabaseBackend, columns: usize) -> usize {
    let max_variables = if backend == DatabaseBackend::Sqlite {
        SQLITE_MAX_VARIABLES
    } else {
        POSTGRES_MAX_VARIABLES
    };
    (max_variables / columns.max(1)).max(1)
}

/// 把一块网卡的上一次读数和总流量转成 `traffic_current_total` 的写入模型。
///
/// - `uuid_id`: 设备编号
/// - `interface_name`: 网卡名
/// - `reading`: 上一次的读数
/// - `total`: 总流量
fn current_total_model(
    uuid_id: i16,
    interface_name: &str,
    reading: &NetworkInterfaceReading,
    total: Traffic,
) -> traffic_current_total::ActiveModel {
    traffic_current_total::ActiveModel {
        id: ActiveValue::default(),
        uuid_id: Set(uuid_id),
        interface_name: Set(interface_name.to_owned()),
        boot_id: Set(reading.boot_id.clone()),
        ifindex: Set(reading.ifindex.map(u32::cast_signed)),
        counter_received: Set(reading.counter_received.cast_signed()),
        counter_transmitted: Set(reading.counter_transmitted.cast_signed()),
        report_time: Set(reading.report_time),
        total_received: Set(total.received.cast_signed()),
        total_transmitted: Set(total.transmitted.cast_signed()),
        created_at: Set(reading.created_at),
        updated_at: Set(reading.updated_at),
    }
}

/// 网卡计数器重置的方向。
///
/// 开机标识或网卡编号变化说明整机/整卡重置，两个方向都按重置处理；
/// 只有一个方向的计数器变小（回绕或清零）时只重置该方向，
/// 另一个方向仍按正常差值累加，避免把健康方向的值当成重置后的全量重复计入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CounterReset {
    /// 接收方向按重置处理
    received: bool,
    /// 发送方向按重置处理
    transmitted: bool,
}

impl CounterReset {
    /// 两个方向都不重置
    const NONE: Self = Self {
        received: false,
        transmitted: false,
    };

    /// 两个方向都按重置处理
    const BOTH: Self = Self {
        received: true,
        transmitted: true,
    };

    /// 是否至少有一个方向发生重置
    const fn any(self) -> bool {
        self.received || self.transmitted
    }
}

/// 判断网卡编号是否变化。
///
/// - `prev_reading`: 上一次的读数
/// - `current_reading`: 本次读数
/// - 返回: 两次都有值且不相同时为 `true`
const fn is_ifindex_changed(
    prev_reading: &NetworkInterfaceReading,
    current_reading: &NetworkInterfaceReading,
) -> bool {
    matches!(
        (prev_reading.ifindex, current_reading.ifindex),
        (Some(prev), Some(current)) if prev != current
    )
}

/// 判断是否整机/整卡重置，即开机标识或网卡编号变化。
///
/// - `prev_reading`: 上一次的读数
/// - `current_reading`: 本次读数
/// - 返回: 开机标识或网卡编号任一变化时为 `true`，此时两个方向都应重置
fn is_full_reset(
    prev_reading: &NetworkInterfaceReading,
    current_reading: &NetworkInterfaceReading,
) -> bool {
    is_boot_id_changed(prev_reading, current_reading)
        || is_ifindex_changed(prev_reading, current_reading)
}

/// 按方向判断网卡计数器是否重置。
///
/// - `prev_reading`: 上一次的读数
/// - `current_reading`: 本次读数
/// - 返回: 每个方向是否按重置处理：
///   1. 开机标识或网卡编号变化时两个方向都按重置处理
///   2. 否则只有累计量变小的方向按重置处理
fn detect_counter_reset(
    prev_reading: &NetworkInterfaceReading,
    current_reading: &NetworkInterfaceReading,
) -> CounterReset {
    if is_full_reset(prev_reading, current_reading) {
        return CounterReset::BOTH;
    }

    CounterReset {
        received: current_reading.counter_received < prev_reading.counter_received,
        transmitted: current_reading.counter_transmitted < prev_reading.counter_transmitted,
    }
}

/// 判断开机标识是否变化，即 VPS 是否重启过。
///
/// - `prev_reading`: 上一次的读数
/// - `current_reading`: 本次读数
/// - 返回: 两次都有值且不相同时为 `true`
fn is_boot_id_changed(
    prev_reading: &NetworkInterfaceReading,
    current_reading: &NetworkInterfaceReading,
) -> bool {
    matches!(
        (&prev_reading.boot_id, &current_reading.boot_id),
        (Some(prev), Some(current)) if prev != current
    )
}

/// 计算单个方向的流量增量。
///
/// - `prev`: 上一次的累计值
/// - `current`: 本次累计值
/// - `reset`: 该方向是否按重置处理
/// - 返回: 重置时取本次累计值，否则取差值（未重置时不会变小，`saturating_sub` 仅作防御）
const fn increase_for_direction(prev: u64, current: u64, reset: bool) -> u64 {
    if reset {
        current
    } else {
        current.saturating_sub(prev)
    }
}

/// 计算本次流量增量。
///
/// - `prev_reading`: 上一次的读数，首次见到该网卡时为 `None`
/// - `current_reading`: 本次读数
/// - `reset`: 每个方向是否按重置处理
/// - 返回: 首次见到时为 0；某个方向发生重置时该方向取本次计数器值，否则取本次与上一次的差
///
/// 首次见到时计数器里是开始统计之前的流量（如开机以来），不计入，从这一刻开始统计
const fn compute_traffic_increase(
    prev_reading: Option<&NetworkInterfaceReading>,
    current_reading: &NetworkInterfaceReading,
    reset: CounterReset,
) -> Traffic {
    match prev_reading {
        None => Traffic {
            received: 0,
            transmitted: 0,
        },
        Some(prev) => Traffic {
            received: increase_for_direction(
                prev.counter_received,
                current_reading.counter_received,
                reset.received,
            ),
            transmitted: increase_for_direction(
                prev.counter_transmitted,
                current_reading.counter_transmitted,
                reset.transmitted,
            ),
        },
    }
}

/// 判断重置前是否可能丢失数据。
///
/// - `uuid_id`: 设备编号
/// - `prev_reading`: 重置前上一次的读数
/// - `reset_at`: 重置时刻（毫秒时间戳）；重启时为开机时间（收到时间 − 已开机时长），其他情况为收到时间
/// - 返回: `prev_reading.updated_at` 到 `reset_at` 超过 `POSSIBLE_DATA_LOSS_THRESHOLD_MS` 时返回该时间段
fn detect_possible_data_loss(
    uuid_id: i16,
    prev_reading: &NetworkInterfaceReading,
    reset_at: i64,
) -> Option<traffic_possible_data_loss::ActiveModel> {
    if reset_at - prev_reading.updated_at <= POSSIBLE_DATA_LOSS_THRESHOLD_MS {
        return None;
    }

    Some(traffic_possible_data_loss::ActiveModel {
        id: ActiveValue::default(),
        uuid_id: Set(uuid_id),
        start_time: Set(prev_reading.updated_at),
        end_time: Set(reset_at),
    })
}

/// 建一个只有流量统计三张表的内存 `SQLite`，供流量统计和流量查询的测试使用。
///
/// 建表语句取自迁移在 `SQLite` 上的实际结果。
#[cfg(test)]
pub(crate) async fn traffic_tables_on_sqlite() -> sea_orm::DatabaseConnection {
    // 内存库每个连接各自独立，只用一个连接
    let mut options = ConnectOptions::new("sqlite::memory:");
    options.max_connections(1);
    let db = Database::connect(options).await.expect("connect sqlite");
    db.execute_unprepared(
        r#"
        CREATE TABLE "traffic_snapshot" ( "id" integer NOT NULL PRIMARY KEY AUTOINCREMENT, "uuid_id" smallint NOT NULL, "interface_name" varchar NOT NULL, "snapshot_time" integer NOT NULL, "total_received" integer NOT NULL, "total_transmitted" integer NOT NULL );
        CREATE UNIQUE INDEX "idx-traffic_snapshot-uuid_id-interface_name-snapshot_time" ON "traffic_snapshot" ("uuid_id", "interface_name", "snapshot_time");
        CREATE TABLE "traffic_current_total" ( "id" integer NOT NULL PRIMARY KEY AUTOINCREMENT, "uuid_id" smallint NOT NULL, "interface_name" varchar NOT NULL, "boot_id" varchar NULL, "ifindex" integer NULL, "counter_received" integer NOT NULL, "counter_transmitted" integer NOT NULL, "report_time" integer NOT NULL, "total_received" integer NOT NULL, "total_transmitted" integer NOT NULL, "created_at" integer NOT NULL, "updated_at" integer NOT NULL );
        CREATE UNIQUE INDEX "idx-traffic_current_total-uuid_id-interface_name-unique" ON "traffic_current_total" ("uuid_id", "interface_name");
        CREATE TABLE "traffic_possible_data_loss" ( "id" integer NOT NULL PRIMARY KEY AUTOINCREMENT, "uuid_id" smallint NOT NULL, "start_time" integer NOT NULL, "end_time" integer NOT NULL );
        "#,
    )
    .await
    .expect("create traffic tables");
    db
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_structure::{
        DynamicCPUData, DynamicLoadData, DynamicMonitoringData, DynamicNetworkData,
        DynamicPerNetworkInterfaceData, DynamicRamData, DynamicSystemData,
    };
    use sea_orm::{PaginatorTrait, Set};
    use std::sync::Arc;

    const GB: u64 = 1024 * 1024 * 1024;
    const MINUTE: i64 = 60 * 1000;
    /// 2026-09-27 10:00:00 UTC
    const TEN_OCLOCK: i64 = 1_790_503_200_000;

    /// 构造一块网卡的上报数据
    fn interface(
        name: &str,
        total_received: u64,
        is_outlet: Option<bool>,
    ) -> DynamicPerNetworkInterfaceData {
        DynamicPerNetworkInterfaceData {
            interface_name: name.to_owned(),
            total_received,
            total_transmitted: 0,
            receive_speed: 0,
            transmit_speed: 0,
            ifindex: Some(2),
            is_outlet,
        }
    }

    /// 构造一块网卡的上报数据，接收和发送计数器都给全
    fn interface_counters(
        name: &str,
        receiver: u64,
        transmitted: u64,
        ifindex: Option<u32>,
    ) -> DynamicPerNetworkInterfaceData {
        DynamicPerNetworkInterfaceData {
            interface_name: name.to_owned(),
            total_received: receiver,
            total_transmitted: transmitted,
            receive_speed: 0,
            transmit_speed: 0,
            ifindex,
            is_outlet: Some(true),
        }
    }

    /// 构造一条动态监控上报，只关心采集时间、已开机时长、开机标识和网卡
    fn report(
        time: i64,
        uptime_secs: u64,
        boot_id: &str,
        interfaces: Vec<DynamicPerNetworkInterfaceData>,
    ) -> DynamicMonitoringData {
        DynamicMonitoringData {
            uuid: uuid::Uuid::nil(),
            time: time.cast_unsigned(),
            cpu: DynamicCPUData {
                per_core: Arc::new(Vec::new()),
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
                uptime: uptime_secs,
                process_count: 0,
                boot_id: Some(boot_id.to_owned()),
            },
            disk: Arc::new(Vec::new()),
            network: DynamicNetworkData {
                interfaces: Arc::new(interfaces),
                udp_connections: 0,
                tcp_connections: 0,
            },
            gpu: Arc::new(Vec::new()),
        }
    }

    /// 取出某块网卡的总接收量
    fn total_received(state: &State, name: &str) -> u64 {
        state.totals[&(1, name.to_owned())].received
    }

    #[test]
    fn first_report_starts_from_zero() {
        let mut state = State::default();
        let data = report(
            TEN_OCLOCK,
            3600,
            "a",
            vec![interface("eth0", 30 * GB, Some(true))],
        );
        state.apply_report(1, &data, TEN_OCLOCK);
        assert_eq!(total_received(&state, "eth0"), 0);
    }

    #[test]
    fn increases_accumulate_across_reports() {
        let mut state = State::default();
        let at = |minutes| TEN_OCLOCK + minutes * MINUTE;
        for (minutes, counter) in [(1, 10 * GB), (2, 11 * GB), (3, 13 * GB)] {
            let data = report(
                at(minutes),
                3600,
                "a",
                vec![interface("eth0", counter, Some(true))],
            );
            state.apply_report(1, &data, at(minutes));
        }
        assert_eq!(total_received(&state, "eth0"), 3 * GB);
    }

    #[test]
    fn current_totals_lists_outlet_interfaces_with_totals_and_update_time() {
        let mut state = State::default();
        for (time, eth0, docker0) in [
            (TEN_OCLOCK, 10 * GB, 5 * GB),
            (TEN_OCLOCK + MINUTE, 12 * GB, 8 * GB),
        ] {
            let data = report(
                time,
                3600,
                "a",
                vec![
                    interface("eth0", eth0, Some(true)),
                    interface("docker0", docker0, Some(false)),
                ],
            );
            state.apply_report(1, &data, time);
        }

        let totals = state.current_totals();
        assert_eq!(
            totals,
            [InterfaceCurrentTotal {
                uuid_id: 1,
                interface_name: "eth0".to_owned(),
                total_received: 2 * GB,
                total_transmitted: 0,
                updated_at: TEN_OCLOCK + MINUTE,
            }]
        );
    }

    #[test]
    fn late_report_is_dropped() {
        let mut state = State::default();
        for (time, counter) in [(TEN_OCLOCK, 18 * GB), (TEN_OCLOCK + MINUTE, 20 * GB)] {
            let data = report(
                time,
                3600,
                "a",
                vec![interface("eth0", counter, Some(true))],
            );
            state.apply_report(1, &data, time);
        }
        // 采集时间更早的数据晚到，计数器更小，不能被当成重置
        let data = report(
            TEN_OCLOCK + 30_000,
            3600,
            "a",
            vec![interface("eth0", 19 * GB, Some(true))],
        );
        state.apply_report(1, &data, TEN_OCLOCK + 2 * MINUTE);
        assert_eq!(total_received(&state, "eth0"), 2 * GB);
    }

    #[test]
    fn old_agent_without_is_outlet_is_skipped() {
        let mut state = State::default();
        let data = report(
            TEN_OCLOCK,
            3600,
            "a",
            vec![interface("eth0", 10 * GB, None)],
        );
        state.apply_report(1, &data, TEN_OCLOCK);
        assert!(state.totals.is_empty());
    }

    #[test]
    fn non_outlet_interfaces_are_ignored() {
        let mut state = State::default();
        for (time, eth0, docker0) in [
            (TEN_OCLOCK, 10 * GB, 5 * GB),
            (TEN_OCLOCK + MINUTE, 12 * GB, 8 * GB),
        ] {
            let data = report(
                time,
                3600,
                "a",
                vec![
                    interface("eth0", eth0, Some(true)),
                    interface("docker0", docker0, Some(false)),
                ],
            );
            state.apply_report(1, &data, time);
        }
        assert_eq!(state.totals.len(), 1);
        assert_eq!(total_received(&state, "eth0"), 2 * GB);
    }

    #[test]
    fn quick_reboot_records_no_possible_data_loss() {
        let mut state = State::default();
        let data = report(
            TEN_OCLOCK,
            3600,
            "a",
            vec![interface("eth0", 50 * GB, Some(true))],
        );
        state.apply_report(1, &data, TEN_OCLOCK);
        // 1 分钟后重启完成并上报，已开机 30 秒
        let at = TEN_OCLOCK + MINUTE;
        let data = report(at, 30, "b", vec![interface("eth0", GB, Some(true))]);
        state.apply_report(1, &data, at);
        // 第一次上报不计入，重启后计数器从 0 开始的 1GB 计入
        assert_eq!(total_received(&state, "eth0"), GB);
        assert!(state.pending_possible_data_losses.is_empty());
    }

    #[test]
    fn long_gap_before_reboot_records_one_loss_ending_at_boot() {
        let mut state = State::default();
        let interfaces = |eth0, eth1| {
            vec![
                interface("eth0", eth0, Some(true)),
                interface("eth1", eth1, Some(true)),
            ]
        };
        let data = report(TEN_OCLOCK, 3600, "a", interfaces(50 * GB, 5 * GB));
        state.apply_report(1, &data, TEN_OCLOCK);
        // 2 小时后才收到数据，此时已开机 5 分钟：开机时间 = 11:55
        let at = TEN_OCLOCK + 120 * MINUTE;
        let data = report(at, 300, "b", interfaces(GB, GB));
        state.apply_report(1, &data, at);

        // 两块网卡都重置，但同一台设备同一时间段只记一次
        assert_eq!(state.pending_possible_data_losses.len(), 1);
        let loss = &state.pending_possible_data_losses[0];
        assert_eq!(loss.start_time, Set(TEN_OCLOCK));
        assert_eq!(loss.end_time, Set(TEN_OCLOCK + 115 * MINUTE));
        assert_eq!(total_received(&state, "eth0"), GB);
        assert_eq!(total_received(&state, "eth1"), GB);
    }

    /// 构造一次读数，只关心开机标识、网卡编号和计数器
    fn reading(
        boot_id: Option<&str>,
        ifindex: Option<u32>,
        counter_received: u64,
        counter_transmitted: u64,
    ) -> NetworkInterfaceReading {
        NetworkInterfaceReading {
            boot_id: boot_id.map(str::to_owned),
            ifindex,
            counter_received,
            counter_transmitted,
            report_time: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn counter_not_reset_when_growing() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(2), 51 * GB, 10 * GB);
        assert_eq!(detect_counter_reset(&prev, &current), CounterReset::NONE);
    }

    #[test]
    fn counter_reset_when_boot_id_changes() {
        // 重启后计数器涨回超过旧值，只能靠开机标识发现
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("b"), Some(2), 60 * GB, 20 * GB);
        assert_eq!(detect_counter_reset(&prev, &current), CounterReset::BOTH);
    }

    #[test]
    fn counter_reset_when_ifindex_changes() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(5), 60 * GB, 20 * GB);
        assert_eq!(detect_counter_reset(&prev, &current), CounterReset::BOTH);
    }

    #[test]
    fn counter_reset_when_received_or_transmitted_decreases() {
        let prev = reading(None, None, 50 * GB, 10 * GB);
        // 只有接收方向变小：只重置接收方向
        assert_eq!(
            detect_counter_reset(&prev, &reading(None, None, GB, 20 * GB)),
            CounterReset {
                received: true,
                transmitted: false,
            }
        );
        // 只有发送方向变小：只重置发送方向
        assert_eq!(
            detect_counter_reset(&prev, &reading(None, None, 60 * GB, GB)),
            CounterReset {
                received: false,
                transmitted: true,
            }
        );
    }

    #[test]
    fn increase_resets_only_the_wrapped_direction() {
        // 只有接收计数器回绕：接收取全量，发送仍取差值，不能把发送值当成重置后的全量
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(2), GB, 15 * GB);
        let reset = detect_counter_reset(&prev, &current);
        assert_eq!(
            reset,
            CounterReset {
                received: true,
                transmitted: false,
            }
        );
        let increase = compute_traffic_increase(Some(&prev), &current, reset);
        assert_eq!(increase.received, GB);
        assert_eq!(increase.transmitted, 5 * GB);
    }

    #[test]
    fn bidirectional_wrap_resets_both_directions() {
        let prev = reading(None, None, 50 * GB, 10 * GB);
        let current = reading(None, None, GB, 2 * GB);
        let reset = detect_counter_reset(&prev, &current);
        assert_eq!(reset, CounterReset::BOTH);
        let increase = compute_traffic_increase(Some(&prev), &current, reset);
        assert_eq!(increase.received, GB);
        assert_eq!(increase.transmitted, 2 * GB);
    }

    #[test]
    fn one_directional_reset_keeps_other_direction_accumulating() {
        let mut state = State::default();
        let at = |minutes| TEN_OCLOCK + minutes * MINUTE;
        // 首次上报：接收 50GB、发送 10GB，不计入
        let data = report(
            at(0),
            3600,
            "a",
            vec![interface_counters("eth0", 50 * GB, 10 * GB, Some(2))],
        );
        state.apply_report(1, &data, at(0));
        // 接收计数器回绕到 1GB，发送正常涨到 15GB：只重置接收方向，发送仍按差值累加
        let data = report(
            at(1),
            3600,
            "a",
            vec![interface_counters("eth0", GB, 15 * GB, Some(2))],
        );
        state.apply_report(1, &data, at(1));
        assert_eq!(total_received(&state, "eth0"), GB);
        assert_eq!(state.totals[&(1, "eth0".to_owned())].transmitted, 5 * GB);
        assert!(state.pending_possible_data_losses.is_empty());
    }

    #[test]
    fn missing_boot_id_or_ifindex_is_not_compared() {
        // 一边为空时跳过该条，不能误判为重置
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        assert_eq!(
            detect_counter_reset(&prev, &reading(None, None, 51 * GB, 10 * GB)),
            CounterReset::NONE
        );
        let prev = reading(None, None, 50 * GB, 10 * GB);
        assert_eq!(
            detect_counter_reset(&prev, &reading(Some("a"), Some(2), 51 * GB, 10 * GB)),
            CounterReset::NONE
        );
    }

    #[test]
    fn increase_is_difference_when_not_reset() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(2), 53 * GB, 11 * GB);
        let increase = compute_traffic_increase(Some(&prev), &current, CounterReset::NONE);
        assert_eq!(increase.received, 3 * GB);
        assert_eq!(increase.transmitted, GB);
    }

    #[test]
    fn increase_is_current_counter_when_reset() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("b"), Some(2), 2 * GB, GB);
        let increase = compute_traffic_increase(Some(&prev), &current, CounterReset::BOTH);
        assert_eq!(increase.received, 2 * GB);
        assert_eq!(increase.transmitted, GB);
    }

    #[test]
    fn increase_is_zero_when_first_seen() {
        // 首次见到时计数器里是开始统计之前的流量，不计入
        let current = reading(Some("a"), Some(2), 30 * GB, 5 * GB);
        let increase = compute_traffic_increase(None, &current, CounterReset::NONE);
        assert_eq!(increase.received, 0);
        assert_eq!(increase.transmitted, 0);
    }

    #[test]
    fn design_example_total_across_resets() {
        // 设计文档的例子：开机时就开始统计，第 1 次开机跑到 50GB 后重启，
        // 第 2 次跑到 30GB 后网卡重建，现在 12GB，总计 92GB
        let readings = [
            reading(Some("boot1"), Some(2), 0, 0),
            reading(Some("boot1"), Some(2), 50 * GB, 0),
            reading(Some("boot2"), Some(2), 30 * GB, 0),
            reading(Some("boot2"), Some(7), 12 * GB, 0),
        ];
        let mut total = 0;
        let mut prev: Option<&NetworkInterfaceReading> = None;
        for current in &readings {
            let reset = prev.map_or(CounterReset::NONE, |p| detect_counter_reset(p, current));
            total += compute_traffic_increase(prev, current, reset).received;
            prev = Some(current);
        }
        assert_eq!(total, 92 * GB);
    }

    #[test]
    fn no_possible_data_loss_within_threshold() {
        let mut prev = reading(Some("a"), Some(2), 0, 0);
        prev.updated_at = 1_000_000;
        let reset_at = prev.updated_at + POSSIBLE_DATA_LOSS_THRESHOLD_MS;
        assert!(detect_possible_data_loss(1, &prev, reset_at).is_none());
    }

    #[test]
    fn possible_data_loss_beyond_threshold() {
        let mut prev = reading(Some("a"), Some(2), 0, 0);
        prev.updated_at = 1_000_000;
        let reset_at = prev.updated_at + POSSIBLE_DATA_LOSS_THRESHOLD_MS + 1;
        let loss = detect_possible_data_loss(7, &prev, reset_at).expect("should record loss");
        assert_eq!(loss.uuid_id, Set(7));
        assert_eq!(loss.start_time, Set(prev.updated_at));
        assert_eq!(loss.end_time, Set(reset_at));
    }

    #[tokio::test]
    async fn write_to_db_updates_current_totals_and_inserts_losses() {
        let db = traffic_tables_on_sqlite().await;
        let loss = traffic_possible_data_loss::ActiveModel {
            id: ActiveValue::default(),
            uuid_id: Set(1),
            start_time: Set(TEN_OCLOCK),
            end_time: Set(TEN_OCLOCK + 60 * MINUTE),
        };
        let reading = reading(Some("a"), Some(2), 50, 5);
        // 200 块网卡，超过 SQLite 单条 SQL 能写的行数，会分批写入
        let current_totals = |received: u64| -> Vec<_> {
            (0..200)
                .map(|i| {
                    let total = Traffic {
                        received,
                        transmitted: 0,
                    };
                    current_total_model(1, &format!("eth{i}"), &reading, total)
                })
                .collect()
        };

        write_to_db(&db, current_totals(100), vec![loss])
            .await
            .expect("first write");
        // 同一块网卡再写一次，当前总流量应被更新而不是重复插入
        write_to_db(&db, current_totals(300), Vec::new())
            .await
            .expect("second write");

        let loss_count = traffic_possible_data_loss::Entity::find()
            .count(&db)
            .await
            .unwrap();
        assert_eq!(loss_count, 1);
        let rows = traffic_current_total::Entity::find()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 200);
        assert!(rows.iter().all(|row| row.total_received == 300));
    }
}
