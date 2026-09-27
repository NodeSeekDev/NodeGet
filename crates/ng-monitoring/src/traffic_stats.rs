//! 流量统计。
//!
//! 根据 Agent 上报的出口网卡累计值，维护每块网卡的总流量（跨重启累加），
//! 每进入一个新的 15 分钟记一次总流量快照，定时写入数据库。
//! 周期流量 = 结束时刻总流量 − 开始时刻总流量。
//! 由 `report_dynamic` 调用 `update_total_traffic`；快照供流量查询使用。

// 骨架阶段尚未接入调用方，接入后删除
#![allow(unused_variables, clippy::unused_async)]

use crate::data_structure::DynamicMonitoringData;
use ng_db::entity::{traffic_possible_data_loss, traffic_snapshot};
use sea_orm::{ActiveValue, Set};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// 快照间隔（毫秒），15 分钟
const SNAPSHOT_INTERVAL_MS: i64 = 15 * 60 * 1000;
/// 判定可能丢失数据的最短间隔（毫秒），10 分钟
const POSSIBLE_DATA_LOSS_THRESHOLD_MS: i64 = 10 * 60 * 1000;
/// 定时写库间隔（毫秒）
const FLUSH_INTERVAL_MS: u64 = 60 * 1000;

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

/// 流量统计的内存状态
#[derive(Debug, Default)]
struct State {
    /// (设备编号, 网卡名) → 上一次的网卡读数
    prev_readings: HashMap<(i16, String), NetworkInterfaceReading>,
    /// (设备编号, 网卡名) → 总流量
    totals: HashMap<(i16, String), Traffic>,
    /// 尚未写库的总流量快照
    pending_snapshots: Vec<traffic_snapshot::ActiveModel>,
    /// 尚未写库的可能丢失数据的时间段
    pending_possible_data_losses: Vec<traffic_possible_data_loss::ActiveModel>,
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
        todo!()
    }

    /// 用一条动态监控上报更新总流量。
    ///
    /// - `uuid_id`: 设备编号（`MonitoringUuidCache` 分配）
    /// - `data`: 动态监控数据
    /// - `received_at`: 服务端收到数据的时间（毫秒时间戳）
    ///
    /// 1. 从 `TRAFFIC_STATS` 取出实例，未初始化或已关闭时直接返回
    /// 2. 有网卡缺少 `is_outlet`（老版本 Agent）时直接跳过
    /// 3. 逐块处理 `is_outlet` 为真的网卡：
    ///    1. 用本次上报组成本次读数，取出上一次的读数；Agent 采集时间早于上一次的直接丢弃
    ///    2. `received_at` 与上一次更新时间不在同一个 15 分钟（`snapshot_time_of`）时，
    ///       以当前总流量记一条快照，时间为 `received_at` 所在 15 分钟的开始时间
    ///    3. 判断是否重置（`is_counter_reset`），算出本次流量增量（`compute_traffic_increase`），加到总流量
    ///    4. 发生重置时判断是否可能丢失数据（`detect_possible_data_loss`），同一台设备同一时间段只记一次
    ///    5. 用本次读数替换上一次的读数
    pub fn update_total_traffic(uuid_id: i16, data: &DynamicMonitoringData, received_at: i64) {
        todo!()
    }

    /// 把内存中的变化写入数据库。
    ///
    /// 1. 取出并清空尚未写库的快照、可能丢失数据的时间段；复制所有网卡的上一次读数和总流量
    /// 2. 在同一个事务中写入：
    ///    1. 插入快照，同一 (设备, 网卡, 快照时间) 已存在则跳过
    ///    2. 写入上一次的读数和总流量，已存在则更新
    ///    3. 插入可能丢失数据的时间段
    /// 3. 写库失败时把取出的快照、可能丢失数据的时间段放回内存，下次重试
    async fn flush(&self) {
        todo!()
    }

    /// 写入剩余数据并关闭，服务端关闭或配置热重载时调用。
    ///
    /// 1. 从 `TRAFFIC_STATS` 取走实例，未初始化时直接返回
    /// 2. 通过 `shutdown` 通知 `flush_loop` 做最后一次写库并退出
    /// 3. 等待 `FLUSH_TASK` 结束，最多 5 秒
    pub async fn flush_and_shutdown() {
        todo!()
    }
}

/// 定时写库循环。
///
/// - `stats`: 流量统计实例
///
/// 1. 每 `FLUSH_INTERVAL_MS` 调用一次 `flush`
/// 2. 收到 `shutdown` 通知时最后调用一次 `flush` 后退出
async fn flush_loop(stats: Arc<TrafficStats>) {
    todo!()
}

/// 判断网卡计数器是否重置。
///
/// - `prev_reading`: 上一次的读数
/// - `current_reading`: 本次读数
/// - 返回: 满足任一条件时为 `true`：
///   1. 开机标识变化（两次都有值时才比较）
///   2. 网卡编号变化（两次都有值时才比较）
///   3. 计数器的累计接收量或累计发送量变小
fn is_counter_reset(
    prev_reading: &NetworkInterfaceReading,
    current_reading: &NetworkInterfaceReading,
) -> bool {
    let boot_id_changed = matches!(
        (&prev_reading.boot_id, &current_reading.boot_id),
        (Some(prev), Some(current)) if prev != current
    );
    let ifindex_changed = matches!(
        (prev_reading.ifindex, current_reading.ifindex),
        (Some(prev), Some(current)) if prev != current
    );
    let counter_decreased = current_reading.counter_received < prev_reading.counter_received
        || current_reading.counter_transmitted < prev_reading.counter_transmitted;

    boot_id_changed || ifindex_changed || counter_decreased
}

/// 计算本次流量增量。
///
/// - `prev_reading`: 上一次的读数，首次见到该网卡时为 `None`
/// - `current_reading`: 本次读数
/// - `reset`: 是否发生重置
/// - 返回: 首次见到或发生重置时为本次计数器值，否则为本次与上一次计数器值的差
const fn compute_traffic_increase(
    prev_reading: Option<&NetworkInterfaceReading>,
    current_reading: &NetworkInterfaceReading,
    reset: bool,
) -> Traffic {
    match prev_reading {
        // 未重置时计数器不会变小，saturating_sub 仅作防御
        Some(prev) if !reset => Traffic {
            received: current_reading
                .counter_received
                .saturating_sub(prev.counter_received),
            transmitted: current_reading
                .counter_transmitted
                .saturating_sub(prev.counter_transmitted),
        },
        _ => Traffic {
            received: current_reading.counter_received,
            transmitted: current_reading.counter_transmitted,
        },
    }
}

/// 计算时间所属的快照时间。
///
/// - `time`: 毫秒时间戳
/// - 返回: 向下取整到 `SNAPSHOT_INTERVAL_MS` 的毫秒时间戳（UTC），即所在 15 分钟的开始时间
const fn snapshot_time_of(time: i64) -> i64 {
    time - time.rem_euclid(SNAPSHOT_INTERVAL_MS)
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

#[cfg(test)]
mod tests {
    use super::{
        NetworkInterfaceReading, POSSIBLE_DATA_LOSS_THRESHOLD_MS, compute_traffic_increase,
        detect_possible_data_loss, is_counter_reset, snapshot_time_of,
    };
    use sea_orm::Set;

    const GB: u64 = 1024 * 1024 * 1024;

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
        assert!(!is_counter_reset(&prev, &current));
    }

    #[test]
    fn counter_reset_when_boot_id_changes() {
        // 重启后计数器涨回超过旧值，只能靠开机标识发现
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("b"), Some(2), 60 * GB, 20 * GB);
        assert!(is_counter_reset(&prev, &current));
    }

    #[test]
    fn counter_reset_when_ifindex_changes() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(5), 60 * GB, 20 * GB);
        assert!(is_counter_reset(&prev, &current));
    }

    #[test]
    fn counter_reset_when_received_or_transmitted_decreases() {
        let prev = reading(None, None, 50 * GB, 10 * GB);
        assert!(is_counter_reset(&prev, &reading(None, None, GB, 20 * GB)));
        assert!(is_counter_reset(&prev, &reading(None, None, 60 * GB, GB)));
    }

    #[test]
    fn missing_boot_id_or_ifindex_is_not_compared() {
        // 一边为空时跳过该条，不能误判为重置
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        assert!(!is_counter_reset(
            &prev,
            &reading(None, None, 51 * GB, 10 * GB)
        ));
        let prev = reading(None, None, 50 * GB, 10 * GB);
        assert!(!is_counter_reset(
            &prev,
            &reading(Some("a"), Some(2), 51 * GB, 10 * GB)
        ));
    }

    #[test]
    fn increase_is_difference_when_not_reset() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("a"), Some(2), 53 * GB, 11 * GB);
        let increase = compute_traffic_increase(Some(&prev), &current, false);
        assert_eq!(increase.received, 3 * GB);
        assert_eq!(increase.transmitted, GB);
    }

    #[test]
    fn increase_is_current_counter_when_reset() {
        let prev = reading(Some("a"), Some(2), 50 * GB, 10 * GB);
        let current = reading(Some("b"), Some(2), 2 * GB, GB);
        let increase = compute_traffic_increase(Some(&prev), &current, true);
        assert_eq!(increase.received, 2 * GB);
        assert_eq!(increase.transmitted, GB);
    }

    #[test]
    fn increase_is_current_counter_when_first_seen() {
        let current = reading(Some("a"), Some(2), 30 * GB, 5 * GB);
        let increase = compute_traffic_increase(None, &current, false);
        assert_eq!(increase.received, 30 * GB);
        assert_eq!(increase.transmitted, 5 * GB);
    }

    #[test]
    fn design_example_total_across_resets() {
        // 设计文档的例子：第 1 次开机跑到 50GB 后重启，第 2 次跑到 30GB 后网卡重建，现在 12GB，总计 92GB
        let readings = [
            reading(Some("boot1"), Some(2), 50 * GB, 0),
            reading(Some("boot2"), Some(2), 30 * GB, 0),
            reading(Some("boot2"), Some(7), 12 * GB, 0),
        ];
        let mut total = 0;
        let mut prev: Option<&NetworkInterfaceReading> = None;
        for current in &readings {
            let reset = prev.is_some_and(|p| is_counter_reset(p, current));
            total += compute_traffic_increase(prev, current, reset).received;
            prev = Some(current);
        }
        assert_eq!(total, 92 * GB);
    }

    #[test]
    fn snapshot_time_aligns_to_fifteen_minutes() {
        // 2026-09-27 10:00:00 UTC
        let ten_oclock = 1_790_503_200_000;
        let minute = 60 * 1000;
        assert_eq!(snapshot_time_of(ten_oclock), ten_oclock);
        assert_eq!(
            snapshot_time_of(ten_oclock + 7 * minute + 23_000),
            ten_oclock
        );
        assert_eq!(
            snapshot_time_of(ten_oclock + 15 * minute),
            ten_oclock + 15 * minute
        );
        assert_eq!(
            snapshot_time_of(ten_oclock + 30 * minute - 1),
            ten_oclock + 15 * minute
        );
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
}
