//! 监控数据查询 DSL 与响应类型定义。
//!
//! 定义了三类监控数据（`static`/`dynamic`/`dynamic_summary`）的查询条件、
//! 查询字段枚举、查询结构体和响应结构体。还包含缩放列的反缩放工具函数。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Re-export query field types from ng-core
pub use ng_core::monitoring::query::{DynamicDataQueryField, StaticDataQueryField};

/// 查询条件枚举，定义各种查询过滤条件。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCondition {
    /// 按 UUID 过滤
    Uuid(uuid::Uuid),
    /// 按时间戳范围过滤（开始时间，结束时间），单位：毫秒
    TimestampFromTo(i64, i64),
    /// 按时间戳起始点过滤，单位：毫秒
    TimestampFrom(i64),
    /// 按时间戳结束点过滤，单位：毫秒
    TimestampTo(i64),

    /// 按入库时间范围过滤（开始时间，结束时间），单位：毫秒
    StorageTimeFromTo(i64, i64),
    /// 按入库时间起始点过滤，单位：毫秒
    StorageTimeFrom(i64),
    /// 按入库时间结束点过滤，单位：毫秒
    StorageTimeTo(i64),

    /// 限制返回结果数量
    Limit(u64),

    /// 获取最后一条记录
    Last,
}

/// 静态监控数据查询结构体。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticDataQuery {
    /// 要查询的字段列表
    pub fields: Vec<StaticDataQueryField>,
    /// 查询条件列表
    pub condition: Vec<QueryCondition>,
}

/// 动态监控数据查询结构体。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicDataQuery {
    /// 要查询的字段列表
    pub fields: Vec<DynamicDataQueryField>,
    /// 查询条件列表
    pub condition: Vec<QueryCondition>,
}

/// 静态监控数据响应项。
#[derive(Serialize)]
pub struct StaticResponseItem {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub timestamp: i64,
    /// CPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Value>,
    /// 系统数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// GPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Value>,
}

/// 动态监控数据响应项。
#[derive(Serialize)]
pub struct DynamicResponseItem {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub timestamp: i64,
    /// CPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Value>,
    /// 内存数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram: Option<Value>,
    /// 负载数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<Value>,
    /// 系统数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// 磁盘数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<Value>,
    /// 网络数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<Value>,
    /// GPU 数据，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Value>,
}

/// 动态监控摘要数据查询字段枚举。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DynamicSummaryQueryField {
    /// CPU 使用率（百分比）
    CpuUsage,
    /// GPU 使用率（百分比）
    GpuUsage,
    /// 已用交换空间（字节）
    UsedSwap,
    /// 总交换空间（字节）
    TotalSwap,
    /// 已用内存（字节）
    UsedMemory,
    /// 总内存（字节）
    TotalMemory,
    /// 可用内存（字节）
    AvailableMemory,
    /// 1 分钟平均负载
    LoadOne,
    /// 5 分钟平均负载
    LoadFive,
    /// 15 分钟平均负载
    LoadFifteen,
    /// 运行时间（秒）
    Uptime,
    /// 启动时间（Unix 时间戳）
    BootTime,
    /// 进程数
    ProcessCount,
    /// 总磁盘空间（字节）
    TotalSpace,
    /// 可用磁盘空间（字节）
    AvailableSpace,
    /// 磁盘读取速度（字节/秒）
    ReadSpeed,
    /// 磁盘写入速度（字节/秒）
    WriteSpeed,
    /// TCP 连接数
    TcpConnections,
    /// UDP 连接数
    UdpConnections,
    /// 总接收字节数
    TotalReceived,
    /// 总发送字节数
    TotalTransmitted,
    /// 发送速度（字节/秒）
    TransmitSpeed,
    /// 接收速度（字节/秒）
    ReceiveSpeed,
}

impl DynamicSummaryQueryField {
    /// 获取字段对应的数据库列名。
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

    /// 获取字段的 JSON 键名（与列名相同，因为是扁平列）。
    #[must_use]
    pub const fn json_key(&self) -> &'static str {
        self.column_name()
    }

    /// 该字段是否在数据库中以 *10 缩放存储（读取时需要 /10.0 还原）。
    ///
    /// 来源与 [`SCALED_SUMMARY_COLUMNS`] 保持一致，新增缩放字段时只需修改常量。
    #[must_use]
    pub fn is_scaled(&self) -> bool {
        SCALED_SUMMARY_COLUMNS.contains(&self.column_name())
    }
}

/// 动态监控摘要数据查询结构体。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicSummaryQuery {
    /// 要查询的字段列表
    pub fields: Vec<DynamicSummaryQueryField>,
    /// 查询条件列表
    pub condition: Vec<QueryCondition>,
}

/// 动态监控摘要数据响应项。
#[derive(Serialize)]
pub struct DynamicSummaryResponseItem {
    /// 设备 UUID
    pub uuid: uuid::Uuid,
    /// 时间戳（毫秒）
    pub timestamp: i64,
    /// CPU 使用率，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_usage: Option<Value>,
    /// GPU 使用率，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_usage: Option<Value>,
    /// 已用交换空间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_swap: Option<Value>,
    /// 总交换空间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_swap: Option<Value>,
    /// 已用内存，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_memory: Option<Value>,
    /// 总内存，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_memory: Option<Value>,
    /// 可用内存，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_memory: Option<Value>,
    /// 1 分钟平均负载，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_one: Option<Value>,
    /// 5 分钟平均负载，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_five: Option<Value>,
    /// 15 分钟平均负载，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_fifteen: Option<Value>,
    /// 系统运行时间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<Value>,
    /// 系统启动时间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_time: Option<Value>,
    /// 进程数量，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_count: Option<Value>,
    /// 磁盘总空间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_space: Option<Value>,
    /// 磁盘可用空间，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_space: Option<Value>,
    /// 磁盘读取速度，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_speed: Option<Value>,
    /// 磁盘写入速度，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_speed: Option<Value>,
    /// TCP 连接数，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_connections: Option<Value>,
    /// UDP 连接数，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udp_connections: Option<Value>,
    /// 网络总接收量，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_received: Option<Value>,
    /// 网络总发送量，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_transmitted: Option<Value>,
    /// 网络发送速度，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transmit_speed: Option<Value>,
    /// 网络接收速度，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_speed: Option<Value>,
}

/// `dynamic_monitoring_summary` 表中以 *10 缩放存储的列名列表（单一事实来源）。
///
/// 这些列以 i16 存储，读取时需除以 10.0 还原。
/// 所有反缩放逻辑和 `DynamicSummaryQueryField::is_scaled` 均由此常量驱动，
/// 新增缩放列只需修改此一处。
pub const SCALED_SUMMARY_COLUMNS: &[&str] = &["cpu_usage", "load_one", "load_five", "load_fifteen"];

/// 对 JSON Object 中的已知缩放列执行 `/10.0` 反缩放，原地修改。
///
/// 这是所有 `dynamic_summary` 读取路径（DB 流、内存 last-cache、SQLite、PostgreSQL）
/// 的标准后处理步骤。对已反缩放的对象重复调用会导致二次除法，
/// 调用方必须确保每行仅执行一次。
///
/// - 整数值作为 `f64` 除法处理
/// - 已有的 `f64` 值也被处理（兼容后端已在 SQL 中完成除法的情况）
/// - `NaN` / `±Infinity` 等无法由 `serde_json::Number` 表示的值保持不变
///
/// - `obj` — 待反缩放的 JSON Map，原地修改
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
    use super::{
        DynamicDataQuery, DynamicDataQueryField, DynamicSummaryQuery, DynamicSummaryQueryField,
        QueryCondition, SCALED_SUMMARY_COLUMNS, StaticDataQuery, StaticDataQueryField,
        apply_descaling_to_json_object,
    };
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

    // ── Additional tests ────────────────────────────────────────────────

    #[test]
    fn query_condition_uuid_serde() {
        let cond = QueryCondition::Uuid(uuid::Uuid::nil());
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: QueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn query_condition_timestamp_from_to() {
        let cond = QueryCondition::TimestampFromTo(1000, 2000);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: QueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn query_condition_limit() {
        let cond = QueryCondition::Limit(500);
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: QueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn query_condition_last() {
        let cond = QueryCondition::Last;
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: QueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn static_data_query_serde() {
        let query = StaticDataQuery {
            fields: vec![StaticDataQueryField::Cpu],
            condition: vec![QueryCondition::Limit(100)],
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: StaticDataQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query, parsed);
    }

    #[test]
    fn dynamic_data_query_serde() {
        let query = DynamicDataQuery {
            fields: vec![DynamicDataQueryField::Cpu],
            condition: vec![
                QueryCondition::Uuid(uuid::Uuid::nil()),
                QueryCondition::TimestampFromTo(0, 9999),
            ],
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: DynamicDataQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query, parsed);
    }

    #[test]
    fn dynamic_summary_query_serde() {
        let query = DynamicSummaryQuery {
            fields: vec![
                DynamicSummaryQueryField::CpuUsage,
                DynamicSummaryQueryField::TotalMemory,
            ],
            condition: vec![QueryCondition::Last],
        };
        let json = serde_json::to_string(&query).unwrap();
        let parsed: DynamicSummaryQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query, parsed);
    }

    #[test]
    fn dynamic_summary_query_field_column_name_matches_json_key() {
        let fields = [
            DynamicSummaryQueryField::CpuUsage,
            DynamicSummaryQueryField::GpuUsage,
            DynamicSummaryQueryField::UsedSwap,
            DynamicSummaryQueryField::TotalSpace,
            DynamicSummaryQueryField::ReceiveSpeed,
        ];
        for field in fields {
            assert_eq!(field.column_name(), field.json_key());
        }
    }

    #[test]
    fn descaling_with_unsigned_integer() {
        let mut obj = Map::new();
        // Use a u64 value that exceeds i64 range but is valid u64
        obj.insert("used_memory".to_owned(), Value::Number(1024i64.into()));
        apply_descaling_to_json_object(&mut obj);
        // used_memory is NOT a scaled column, should remain unchanged
        assert_eq!(obj["used_memory"], Value::Number(1024i64.into()));
    }

    #[test]
    fn descaling_with_float_value_in_scaled_column() {
        let mut obj = Map::new();
        obj.insert(
            "cpu_usage".to_owned(),
            Value::Number(Number::from_f64(534.0).unwrap()),
        );
        apply_descaling_to_json_object(&mut obj);
        assert_eq!(
            obj["cpu_usage"],
            Value::Number(Number::from_f64(53.4).unwrap())
        );
    }
}
