//! 监控数据最新值缓存。
//!
//! 维护三类监控数据（`static`/`dynamic`/`dynamic_summary`）每台设备的最新一条记录，
//! 用于 multi-last 查询的快速路径，避免每次都回表查询数据库。
//! 内部使用 `RwLock<HashMap>` 实现读写分离，锁中毒时自动恢复。
//!
//! ## 性能优化
//!
//! 每条缓存条目同时保存 `serde_json::Value` 和预序列化的 `Arc<str>`：
//! - 全字段查询：直接返回 `Arc<str>`，跳过 Map 构造、键分配、Value 克隆和再序列化
//! - 字段筛选查询：基于 `Value` 构造筛选 Map，行为不变
//!
//! 对于动态摘要类型，`serialized` 存储的是反缩放后的 JSON 字符串，
//! 而 `value` 仍保留缩放值供筛选查询使用（筛选后再由调用方反缩放）。

use crate::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use crate::query::apply_descaling_to_json_object;
use crate::query::{DynamicDataQueryField, DynamicSummaryQueryField, StaticDataQueryField};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::sync::Arc;
use tracing::{debug, trace};
use uuid::Uuid;

/// 全局 `MonitoringLastCache` 单例。
static CACHE: OnceLock<MonitoringLastCache> = OnceLock::new();

/// 缓存条目，同时持有 JSON 值和预序列化字符串。
///
/// - `value`：用于字段筛选查询（从完整对象中提取子集）
/// - `serialized`：预序列化的完整 JSON 字符串，用于全字段查询直接返回；
///   序列化失败时为 `None`，`get_*_last_raw()` 将返回 `None` 而非空字符串
struct CachedEntry {
    /// 完整 JSON 值，用于字段筛选查询
    value: serde_json::Value,
    /// 预序列化的完整 JSON 字符串，用于全字段查询直接返回。
    /// 序列化失败时为 None，避免 Raw 路径返回无效 JSON（空字符串）。
    serialized: Option<Arc<str>>,
}

impl CachedEntry {
    /// 从 JSON Value 创建缓存条目，同时预序列化。
    fn new(value: serde_json::Value) -> Self {
        let serialized = serde_json::to_string(&value)
            .ok()
            .map(|s| Arc::from(s.into_boxed_str()));
        Self { value, serialized }
    }

    /// 从动态摘要 JSON Value 创建缓存条目，预序列化时执行反缩放。
    ///
    /// `value` 保留缩放值供筛选查询使用；`serialized` 存储反缩放后的字符串，
    /// 全字段查询可直接返回而无需再次反缩放。
    fn new_summary(value: serde_json::Value) -> Self {
        // 克隆一份用于反缩放序列化（更新频率远低于查询频率，此处开销可接受）
        let mut descaled = value.clone();
        if let Some(obj) = descaled.as_object_mut() {
            apply_descaling_to_json_object(obj);
        }
        let serialized = serde_json::to_string(&descaled)
            .ok()
            .map(|s| Arc::from(s.into_boxed_str()));
        Self { value, serialized }
    }
}

/// 监控数据最新值缓存，按 UUID 维度存储每类数据的最新一条 JSON 记录。
#[allow(clippy::struct_field_names)]
pub struct MonitoringLastCache {
    /// 静态监控最新值缓存
    static_cache: RwLock<HashMap<Uuid, CachedEntry>>,
    /// 动态监控最新值缓存
    dynamic_cache: RwLock<HashMap<Uuid, CachedEntry>>,
    /// 动态摘要最新值缓存
    dynamic_summary_cache: RwLock<HashMap<Uuid, CachedEntry>>,
}

impl MonitoringLastCache {
    /// 初始化全局单例。
    pub fn init() {
        CACHE.get_or_init(|| Self {
            static_cache: RwLock::new(HashMap::with_capacity(32)),
            dynamic_cache: RwLock::new(HashMap::with_capacity(32)),
            dynamic_summary_cache: RwLock::new(HashMap::with_capacity(32)),
        });
    }

    /// 获取全局单例引用，未初始化时返回 `None`。
    pub fn global() -> Option<&'static Self> {
        CACHE.get()
    }

    /// 直接用预构建的 JSON 值更新静态最新缓存。
    pub fn update_static_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.static_cache).insert(uuid, CachedEntry::new(value));
        debug!(target: "monitoring", %uuid, "Static last-cache updated");
    }

    /// 直接用预构建的 JSON 值更新动态最新缓存。
    pub fn update_dynamic_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.dynamic_cache).insert(uuid, CachedEntry::new(value));
        debug!(target: "monitoring", %uuid, "Dynamic last-cache updated");
    }

    /// 直接用预构建的 JSON 值更新动态摘要最新缓存。
    ///
    /// 内部会额外计算反缩放后的预序列化字符串，供全字段查询直接使用。
    pub fn update_dynamic_summary_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.dynamic_summary_cache).insert(uuid, CachedEntry::new_summary(value));
        debug!(target: "monitoring", %uuid, "Dynamic-summary last-cache updated");
    }

    /// 从原始数据构建 JSON 值并更新静态最新缓存。
    pub fn update_static(&self, uuid: Uuid, timestamp: i64, data: &StaticMonitoringData) {
        let value = build_static_value(uuid, timestamp, data);
        self.update_static_prebuilt(uuid, value);
    }

    /// 从原始数据构建 JSON 值并更新动态最新缓存。
    pub fn update_dynamic(&self, uuid: Uuid, timestamp: i64, data: &DynamicMonitoringData) {
        let value = build_dynamic_value(uuid, timestamp, data);
        self.update_dynamic_prebuilt(uuid, value);
    }

    /// 从原始数据构建 JSON 值并更新动态摘要最新缓存。
    pub fn update_dynamic_summary(
        &self,
        uuid: Uuid,
        timestamp: i64,
        data: &DynamicMonitoringSummaryData,
    ) {
        let value = build_dynamic_summary_value(uuid, timestamp, data);
        self.update_dynamic_summary_prebuilt(uuid, value);
    }

    /// 获取指定 UUID 的静态最新值，仅返回请求的字段。
    ///
    /// - `uuid` — 设备 UUID
    /// - `fields` — 需要的字段列表，始终包含 uuid 和 timestamp
    /// - 返回值 — 筛选后的 JSON Object，缓存未命中时返回 `None`
    pub fn get_static_last(
        &self,
        uuid: &Uuid,
        fields: &[StaticDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.static_cache);
        let entry = guard.get(uuid)?;
        let full_obj = entry.value.as_object()?;
        let filtered = build_filtered_map(full_obj, fields.iter().map(StaticDataQueryField::json_key));
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Static last-cache hit");
        Some(filtered)
    }

    /// 获取指定 UUID 的动态最新值，仅返回请求的字段。
    ///
    /// - `uuid` — 设备 UUID
    /// - `fields` — 需要的字段列表，始终包含 uuid 和 timestamp
    /// - 返回值 — 筛选后的 JSON Object，缓存未命中时返回 `None`
    pub fn get_dynamic_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.dynamic_cache);
        let entry = guard.get(uuid)?;
        let full_obj = entry.value.as_object()?;
        let filtered = build_filtered_map(full_obj, fields.iter().map(DynamicDataQueryField::json_key));
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic last-cache hit");
        Some(filtered)
    }

    /// 获取指定 UUID 的动态摘要最新值。
    ///
    /// - `uuid` — 设备 UUID
    /// - `fields` — 需要的字段列表；为空时返回完整记录
    /// - 返回值 — 筛选后的 JSON Object（缩放值，需由调用方反缩放），缓存未命中时返回 `None`
    pub fn get_dynamic_summary_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicSummaryQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.dynamic_summary_cache);
        let entry = guard.get(uuid)?;
        if fields.is_empty() {
            let cloned = entry.value.clone();
            drop(guard);
            trace!(target: "monitoring", %uuid, field_count = 0, "Dynamic-summary last-cache hit (all fields)");
            return Some(cloned);
        }
        let full_obj = entry.value.as_object()?;
        let filtered = build_filtered_map(full_obj, fields.iter().map(DynamicSummaryQueryField::json_key));
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic-summary last-cache hit");
        Some(filtered)
    }

    /// 获取指定 UUID 的静态最新值的预序列化字符串（全字段）。
    ///
    /// 直接返回缓存的 `Arc<str>`，无需 Map 构造和 Value 克隆。
    /// 缓存未命中或序列化失败时返回 `None`。
    pub fn get_static_last_raw(&self, uuid: &Uuid) -> Option<Arc<str>> {
        let guard = recover_read(&self.static_cache);
        let entry = guard.get(uuid)?;
        let serialized = entry.serialized.as_ref().map(Arc::clone)?;
        drop(guard);
        trace!(target: "monitoring", %uuid, "Static last-cache raw hit");
        Some(serialized)
    }

    /// 获取指定 UUID 的动态最新值的预序列化字符串（全字段）。
    ///
    /// 直接返回缓存的 `Arc<str>`，无需 Map 构造和 Value 克隆。
    /// 缓存未命中或序列化失败时返回 `None`。
    pub fn get_dynamic_last_raw(&self, uuid: &Uuid) -> Option<Arc<str>> {
        let guard = recover_read(&self.dynamic_cache);
        let entry = guard.get(uuid)?;
        let serialized = entry.serialized.as_ref().map(Arc::clone)?;
        drop(guard);
        trace!(target: "monitoring", %uuid, "Dynamic last-cache raw hit");
        Some(serialized)
    }

    /// 获取指定 UUID 的动态摘要最新值的预序列化字符串（全字段，已反缩放）。
    ///
    /// 直接返回缓存的 `Arc<str>`（已反缩放），无需 Map 构造、Value 克隆和反缩放处理。
    /// 缓存未命中或序列化失败时返回 `None`。
    pub fn get_dynamic_summary_last_raw(&self, uuid: &Uuid) -> Option<Arc<str>> {
        let guard = recover_read(&self.dynamic_summary_cache);
        let entry = guard.get(uuid)?;
        let serialized = entry.serialized.as_ref().map(Arc::clone)?;
        drop(guard);
        trace!(target: "monitoring", %uuid, "Dynamic-summary last-cache raw hit");
        Some(serialized)
    }
}

/// 从完整 JSON 对象中提取指定字段，构建筛选后的 Map。
///
/// 始终包含 `uuid` 和 `timestamp` 字段，再添加 `extra_keys` 指定的字段。
/// `serde_json::Map` 底层为 `BTreeMap`（默认 non-preserve-order feature），
/// `insert` 自动按键名字母序排列，无需显式排序。
fn build_filtered_map<'a, I: Iterator<Item = &'a str>>(
    full_obj: &serde_json::Map<String, serde_json::Value>,
    extra_keys: I,
) -> serde_json::Value {
    let mut filtered = serde_json::Map::with_capacity(extra_keys.size_hint().0 + 2);

    if let Some(v) = full_obj.get("uuid") {
        filtered.insert("uuid".to_owned(), v.clone());
    }
    if let Some(v) = full_obj.get("timestamp") {
        filtered.insert("timestamp".to_owned(), v.clone());
    }
    for key in extra_keys {
        if key != "uuid" && key != "timestamp" && let Some(v) = full_obj.get(key) {
            filtered.insert(key.to_owned(), v.clone());
        }
    }

    serde_json::Value::Object(filtered)
}

/// 从 `RwLock` 获取读锁，锁中毒时自动恢复（取走内部数据继续使用）。
fn recover_read<K, V>(
    lock: &RwLock<HashMap<K, V>>,
) -> std::sync::RwLockReadGuard<'_, HashMap<K, V>> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

/// 从 `RwLock` 获取写锁，锁中毒时自动恢复。
fn recover_write<K, V>(
    lock: &RwLock<HashMap<K, V>>,
) -> std::sync::RwLockWriteGuard<'_, HashMap<K, V>> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

/// 从静态监控数据构建用于缓存的 JSON 值。
///
/// - `uuid` — 设备 UUID
/// - `timestamp` — 时间戳（毫秒）
/// - `data` — 静态监控数据引用
/// - 返回值 — 包含 uuid、timestamp、cpu、system、gpu 的 JSON Object
#[must_use]
pub fn build_static_value(
    uuid: Uuid,
    timestamp: i64,
    data: &StaticMonitoringData,
) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(5);
    obj.insert(
        "uuid".to_owned(),
        serde_json::Value::String(uuid.to_string()),
    );
    obj.insert(
        "timestamp".to_owned(),
        serde_json::Value::Number(timestamp.into()),
    );
    if let Ok(v) = serde_json::to_value(&data.cpu) {
        obj.insert("cpu".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.system) {
        obj.insert("system".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.gpu) {
        obj.insert("gpu".to_owned(), v);
    }
    serde_json::Value::Object(obj)
}

/// 从动态监控数据构建用于缓存的 JSON 值。
///
/// - `uuid` — 设备 UUID
/// - `timestamp` — 时间戳（毫秒）
/// - `data` — 动态监控数据引用
/// - 返回值 — 包含 uuid、timestamp、cpu、ram、load、system、disk、network、gpu 的 JSON Object
#[must_use]
pub fn build_dynamic_value(
    uuid: Uuid,
    timestamp: i64,
    data: &DynamicMonitoringData,
) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(9);
    obj.insert(
        "uuid".to_owned(),
        serde_json::Value::String(uuid.to_string()),
    );
    obj.insert(
        "timestamp".to_owned(),
        serde_json::Value::Number(timestamp.into()),
    );
    if let Ok(v) = serde_json::to_value(&data.cpu) {
        obj.insert("cpu".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.ram) {
        obj.insert("ram".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.load) {
        obj.insert("load".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.system) {
        obj.insert("system".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.disk) {
        obj.insert("disk".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.network) {
        obj.insert("network".to_owned(), v);
    }
    if let Ok(v) = serde_json::to_value(&data.gpu) {
        obj.insert("gpu".to_owned(), v);
    }
    serde_json::Value::Object(obj)
}

/// 从动态摘要监控数据构建用于缓存的 JSON 值。
///
/// - `uuid` — 设备 UUID
/// - `timestamp` — 时间戳（毫秒）
/// - `data` — 动态摘要监控数据引用
/// - 返回值 — 包含所有摘要字段的 JSON Object（注意：缩放值保持原始存储格式，反缩放在查询时执行）
#[must_use]
pub fn build_dynamic_summary_value(
    uuid: Uuid,
    timestamp: i64,
    data: &DynamicMonitoringSummaryData,
) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(24);
    obj.insert(
        "uuid".to_owned(),
        serde_json::Value::String(uuid.to_string()),
    );
    obj.insert(
        "timestamp".to_owned(),
        serde_json::Value::Number(timestamp.into()),
    );

    macro_rules! opt_field {
        ($key:literal, $val:expr) => {
            if let Some(v) = $val {
                obj.insert($key.to_owned(), serde_json::Value::Number(v.into()));
            }
        };
    }

    opt_field!("cpu_usage", data.cpu_usage.map(i64::from));
    opt_field!("gpu_usage", data.gpu_usage.map(i64::from));
    opt_field!("used_swap", data.used_swap);
    opt_field!("total_swap", data.total_swap);
    opt_field!("used_memory", data.used_memory);
    opt_field!("total_memory", data.total_memory);
    opt_field!("available_memory", data.available_memory);
    opt_field!("load_one", data.load_one.map(i64::from));
    opt_field!("load_five", data.load_five.map(i64::from));
    opt_field!("load_fifteen", data.load_fifteen.map(i64::from));
    opt_field!("uptime", data.uptime.map(i64::from));
    opt_field!("boot_time", data.boot_time);
    opt_field!("process_count", data.process_count.map(i64::from));
    opt_field!("total_space", data.total_space);
    opt_field!("available_space", data.available_space);
    opt_field!("read_speed", data.read_speed);
    opt_field!("write_speed", data.write_speed);
    opt_field!("tcp_connections", data.tcp_connections.map(i64::from));
    opt_field!("udp_connections", data.udp_connections.map(i64::from));
    opt_field!("total_received", data.total_received);
    opt_field!("total_transmitted", data.total_transmitted);
    opt_field!("transmit_speed", data.transmit_speed);
    opt_field!("receive_speed", data.receive_speed);

    serde_json::Value::Object(obj)
}
