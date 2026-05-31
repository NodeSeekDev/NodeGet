use serde::{Deserialize, Serialize};
use serde_json::Value;

// Re-export query field types from ng-core
pub use ng_core::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};

// 查询条件枚举，定义各种查询过滤条件
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCondition {
    // 按 UUID 过滤
    Uuid(uuid::Uuid),
    // 按时间戳范围过滤（开始时间，结束时间）
    TimestampFromTo(i64, i64), // start, end
    // 按时间戳起始点过滤
    TimestampFrom(i64), // start,
    // 按时间戳结束点过滤
    TimestampTo(i64), // end

    // 按入库时间范围过滤（开始时间，结束时间）
    StorageTimeFromTo(i64, i64), // start, end
    // 按入库时间起始点过滤
    StorageTimeFrom(i64), // start
    // 按入库时间结束点过滤
    StorageTimeTo(i64), // end

    // 限制返回结果数量
    Limit(u64), // limit

    // 获取最后一条记录
    Last,
}

// 静态监控数据查询结构体
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticDataQuery {
    // 要查询的字段列表
    pub fields: Vec<StaticDataQueryField>,
    // 查询条件列表
    pub condition: Vec<QueryCondition>,
}

// 动态监控数据查询结构体
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicDataQuery {
    // 要查询的字段列表
    pub fields: Vec<DynamicDataQueryField>,
    // 查询条件列表
    pub condition: Vec<QueryCondition>,
}

// 静态监控数据响应项结构体
#[derive(Serialize)]
pub struct StaticResponseItem {
    // 设备 UUID
    pub uuid: uuid::Uuid,
    // 时间戳
    pub timestamp: i64,
    // CPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Value>,
    // 系统数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    // GPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Value>,
}

// 动态监控数据响应项结构体
#[derive(Serialize)]
pub struct DynamicResponseItem {
    // 设备 UUID
    pub uuid: uuid::Uuid,
    // 时间戳
    pub timestamp: i64,
    // CPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Value>,
    // 内存数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram: Option<Value>,
    // 负载数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<Value>,
    // 系统数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    // 磁盘数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<Value>,
    // 网络数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<Value>,
    // GPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Value>,
}

// 动态监控摘要数据查询字段枚举
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DynamicSummaryQueryField {
    CpuUsage,
    GpuUsage,
    UsedSwap,
    TotalSwap,
    UsedMemory,
    TotalMemory,
    AvailableMemory,
    LoadOne,
    LoadFive,
    LoadFifteen,
    Uptime,
    BootTime,
    ProcessCount,
    TotalSpace,
    AvailableSpace,
    ReadSpeed,
    WriteSpeed,
    TcpConnections,
    UdpConnections,
    TotalReceived,
    TotalTransmitted,
    TransmitSpeed,
    ReceiveSpeed,
}

impl DynamicSummaryQueryField {
    /// 获取字段对应的数据库列名
    #[must_use]
    pub const fn column_name(&self) -> &'static str {
        match self {
            Self::CpuUsage => "cpu_usage",
            Self::GpuUsage => "gpu_usage",
            Self::UsedSwap => "used_swap",
            Self::TotalSwap => "total_swap",
            Self::UsedMemory => "used_memory",
            Self::TotalMemory => "total_memory",
            Self::AvailableMemory => "available_memory",
            Self::LoadOne => "load_one",
            Self::LoadFive => "load_five",
            Self::LoadFifteen => "load_fifteen",
            Self::Uptime => "uptime",
            Self::BootTime => "boot_time",
            Self::ProcessCount => "process_count",
            Self::TotalSpace => "total_space",
            Self::AvailableSpace => "available_space",
            Self::ReadSpeed => "read_speed",
            Self::WriteSpeed => "write_speed",
            Self::TcpConnections => "tcp_connections",
            Self::UdpConnections => "udp_connections",
            Self::TotalReceived => "total_received",
            Self::TotalTransmitted => "total_transmitted",
            Self::TransmitSpeed => "transmit_speed",
            Self::ReceiveSpeed => "receive_speed",
        }
    }

    /// 获取字段的 JSON 键名（与列名相同，因为是扁平列）
    #[must_use]
    pub const fn json_key(&self) -> &'static str {
        self.column_name()
    }

    /// 该字段是否在数据库中以 *10 缩放存储（读取时需要 /10.0 还原）
    ///
    /// 来源与 [`SCALED_SUMMARY_COLUMNS`] 保持一致，新增缩放字段时只需修改常量。
    #[must_use]
    pub fn is_scaled(&self) -> bool {
        SCALED_SUMMARY_COLUMNS.contains(&self.column_name())
    }
}

// 动态监控摘要数据查询结构体
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicSummaryQuery {
    pub fields: Vec<DynamicSummaryQueryField>,
    pub condition: Vec<QueryCondition>,
}

// 动态监控摘要数据响应项结构体
#[derive(Serialize)]
pub struct DynamicSummaryResponseItem {
    pub uuid: uuid::Uuid,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_usage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_usage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_swap: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_swap: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_memory: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_memory: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_memory: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_one: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_five: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_fifteen: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_time: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_count: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_space: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_space: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_speed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_speed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_connections: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udp_connections: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_received: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_transmitted: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transmit_speed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_speed: Option<Value>,
}

/// Single source of truth for the list of `dynamic_monitoring_summary` columns
/// that are stored as `*10`-scaled `i16` integers and must be divided by
/// `10.0` on read.
///
/// All descaling helpers and `DynamicSummaryQueryField::is_scaled` derive from
/// this constant, so adding a new scaled column only requires one edit.
pub const SCALED_SUMMARY_COLUMNS: &[&str] = &["cpu_usage", "load_one", "load_five", "load_fifteen"];

/// Apply `/10.0` descaling to known scaled columns in `obj`, in place.
///
/// This is the canonical post-processing step used by every `dynamic_summary`
/// read path (DB stream, in-memory last-cache, `SQLite`, `PostgreSQL`).
/// Running it on an already-descaled object would double-divide, so call
/// sites must ensure it runs exactly once per row.
///
/// Integer values are divided as `f64`; existing `f64` values are also
/// handled (for forwards compatibility with backends that already apply the
/// division in SQL). `NaN` / `±Infinity` values that cannot be represented by
/// `serde_json::Number` are left untouched rather than silently dropped.
pub fn apply_descaling_to_json_object(obj: &mut serde_json::Map<String, serde_json::Value>) {
    for key in SCALED_SUMMARY_COLUMNS {
        if let Some(val) = obj.get_mut(*key)
            && let serde_json::Value::Number(n) = val
        {
            // Resolve the numeric value to an `f64` regardless of how
            // `serde_json` stored it (signed int, unsigned int, or already
            // a float), then divide by 10. A `None` at this stage means the
            // value was not representable (e.g. `NaN`), in which case we
            // leave the JSON value untouched.
            let raw: Option<f64> = n
                .as_i64()
                .map(|i| {
                    #[allow(clippy::cast_precision_loss)]
                    let f = i as f64;
                    f
                })
                .or_else(|| {
                    n.as_u64().map(|u| {
                        #[allow(clippy::cast_precision_loss)]
                        let f = u as f64;
                        f
                    })
                })
                .or_else(|| n.as_f64());

            if let Some(f) = raw
                && let Some(scaled) = serde_json::Number::from_f64(f / 10.0)
            {
                *val = serde_json::Value::Number(scaled);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DynamicSummaryQueryField, SCALED_SUMMARY_COLUMNS, apply_descaling_to_json_object};
    use serde_json::{Map, Number, Value};

    #[test]
    fn scaled_fields_match_single_source_of_truth() {
        let scaled_from_const: std::collections::HashSet<&str> =
            SCALED_SUMMARY_COLUMNS.iter().copied().collect();

        let all_fields = [
            DynamicSummaryQueryField::CpuUsage,
            DynamicSummaryQueryField::GpuUsage,
            DynamicSummaryQueryField::UsedSwap,
            DynamicSummaryQueryField::TotalSwap,
            DynamicSummaryQueryField::UsedMemory,
            DynamicSummaryQueryField::TotalMemory,
            DynamicSummaryQueryField::AvailableMemory,
            DynamicSummaryQueryField::LoadOne,
            DynamicSummaryQueryField::LoadFive,
            DynamicSummaryQueryField::LoadFifteen,
            DynamicSummaryQueryField::Uptime,
            DynamicSummaryQueryField::BootTime,
            DynamicSummaryQueryField::ProcessCount,
            DynamicSummaryQueryField::TotalSpace,
            DynamicSummaryQueryField::AvailableSpace,
            DynamicSummaryQueryField::ReadSpeed,
            DynamicSummaryQueryField::WriteSpeed,
            DynamicSummaryQueryField::TcpConnections,
            DynamicSummaryQueryField::UdpConnections,
            DynamicSummaryQueryField::TotalReceived,
            DynamicSummaryQueryField::TotalTransmitted,
            DynamicSummaryQueryField::TransmitSpeed,
            DynamicSummaryQueryField::ReceiveSpeed,
        ];

        for field in all_fields {
            let expected = scaled_from_const.contains(field.column_name());
            assert_eq!(
                field.is_scaled(),
                expected,
                "field `{}` is_scaled() does not match SCALED_SUMMARY_COLUMNS",
                field.column_name()
            );
        }
    }

    #[test]
    fn descaling_divides_integer_scaled_fields_by_ten() {
        let mut obj = Map::new();
        obj.insert("cpu_usage".to_owned(), Value::Number(534i64.into()));
        obj.insert("load_one".to_owned(), Value::Number(15i64.into()));
        obj.insert("load_five".to_owned(), Value::Number(7i64.into()));
        obj.insert("load_fifteen".to_owned(), Value::Number(3i64.into()));
        obj.insert("used_memory".to_owned(), Value::Number(1024i64.into()));

        apply_descaling_to_json_object(&mut obj);

        assert_eq!(
            obj["cpu_usage"],
            Value::Number(Number::from_f64(53.4).unwrap())
        );
        assert_eq!(
            obj["load_one"],
            Value::Number(Number::from_f64(1.5).unwrap())
        );
        assert_eq!(
            obj["load_five"],
            Value::Number(Number::from_f64(0.7).unwrap())
        );
        assert_eq!(
            obj["load_fifteen"],
            Value::Number(Number::from_f64(0.3).unwrap())
        );
        assert_eq!(obj["used_memory"], Value::Number(1024i64.into()));
    }

    #[test]
    fn descaling_handles_float_input_idempotently_once() {
        let mut obj = Map::new();
        obj.insert(
            "cpu_usage".to_owned(),
            Value::Number(Number::from_f64(10.0).unwrap()),
        );

        apply_descaling_to_json_object(&mut obj);

        assert_eq!(
            obj["cpu_usage"],
            Value::Number(Number::from_f64(1.0).unwrap())
        );
    }

    #[test]
    fn descaling_leaves_missing_keys_alone() {
        let mut obj = Map::new();
        obj.insert("used_memory".to_owned(), Value::Number(1_234_567i64.into()));

        apply_descaling_to_json_object(&mut obj);

        assert_eq!(obj.len(), 1);
        assert_eq!(obj["used_memory"], Value::Number(1_234_567i64.into()));
    }

    #[test]
    fn descaling_skips_null_values() {
        let mut obj = Map::new();
        obj.insert("cpu_usage".to_owned(), Value::Null);

        apply_descaling_to_json_object(&mut obj);

        assert_eq!(obj["cpu_usage"], Value::Null);
    }
}
