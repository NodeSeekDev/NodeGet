//! `agent.query_traffic` RPC 实现。
//!
//! 查询一台设备在指定时间段内的流量。
//! 数据来自 `traffic_snapshot`、`traffic_current_total`、`traffic_possible_data_loss` 三张表，
//! 不调用 `TrafficStats`。

// 骨架阶段尚未实现，实现后删除
#![allow(unused_variables, clippy::unused_async)]

use crate::query::{InterfaceTrafficItem, PossibleDataLossItem, TrafficQuery, TrafficSnapshotItem};
use jsonrpsee::core::RpcResult;
use serde_json::value::RawValue;

/// `detail` 的最长时间范围（毫秒），92 天
const MAX_DETAIL_RANGE_MS: i64 = 92 * 24 * 60 * 60 * 1000;

/// 查询流量统计。
///
/// - `token` — 身份认证凭据
/// - `query` — 查询参数
/// - 返回值 — `granularity` 为 `total` 时返回 `TrafficTotalResponse`，为 `detail` 时返回 `TrafficDetailResponse`
///
/// 内部步骤：
/// 1. 解析 Token 并验证 `DynamicMonitoring::Read(Network)` 权限（`Scope`: `AgentUuid`）
/// 2. 通过 `MonitoringUuidCache` 把 UUID 转为 `uuid_id`
/// 3. 开始时间晚于结束时间时返回错误
/// 4. 按 `granularity` 查询流量（`query_total` / `query_detail`）
/// 5. 查询可能丢失数据的时间段（`query_possible_data_losses`），一并返回
///
/// # Errors
///
/// - Token 解析失败时返回 `NodegetError::ParseError`
/// - 权限不足时返回 `NodegetError::PermissionDenied`
/// - UUID 未找到时返回 `NodegetError::NotFound`
/// - 开始时间晚于结束时间、`detail` 时间范围超过 `MAX_DETAIL_RANGE_MS` 时返回 `NodegetError::InvalidInput`
/// - 数据库查询失败时返回 `NodegetError::DatabaseError`
pub async fn query_traffic(token: String, query: TrafficQuery) -> RpcResult<Box<RawValue>> {
    todo!()
}

/// 查询时间段内每块网卡的流量合计。
///
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 每块网卡的流量，按网卡名排序
///
/// 每块网卡分别计算：
/// 1. 开始值：开始时间之前（含）最近的一条快照；没有快照或未填开始时间时为 0
/// 2. 结束值：未填结束时间时取 `traffic_current_total`；否则取结束时间之前（含）最近的一条快照
/// 3. 流量 = 结束值 − 开始值
async fn query_total(
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<InterfaceTrafficItem>> {
    todo!()
}

/// 查询时间段内的所有总流量快照。
///
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 开始时间到结束时间（含两端）之间的快照，按网卡名、快照时间排序
///
/// 1. 时间范围超过 `MAX_DETAIL_RANGE_MS` 时返回错误，未填的一端按最早快照时间、当前时间计算
/// 2. 查询范围内的快照
async fn query_detail(
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<TrafficSnapshotItem>> {
    todo!()
}

/// 查询与时间段有重叠的可能丢失数据的时间段。
///
/// - `uuid_id`: 设备编号
/// - `start_time`: 开始时间（毫秒），`None` 表示从最早开始
/// - `end_time`: 结束时间（毫秒），`None` 表示到现在
/// - 返回: 按开始时间排序
async fn query_possible_data_losses(
    uuid_id: i16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<PossibleDataLossItem>> {
    todo!()
}
