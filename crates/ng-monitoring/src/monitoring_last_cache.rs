use crate::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use crate::query::{DynamicDataQueryField, DynamicSummaryQueryField, StaticDataQueryField};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;
use tracing::{debug, trace};
use uuid::Uuid;

static CACHE: OnceLock<MonitoringLastCache> = OnceLock::new();

pub struct MonitoringLastCache {
    static_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
    dynamic_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
    dynamic_summary_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
}

impl MonitoringLastCache {
    pub fn init() {
        CACHE.get_or_init(|| Self {
            static_cache: RwLock::new(HashMap::with_capacity(32)),
            dynamic_cache: RwLock::new(HashMap::with_capacity(32)),
            dynamic_summary_cache: RwLock::new(HashMap::with_capacity(32)),
        });
    }

    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("MonitoringLastCache not initialized — call MonitoringLastCache::init() first")
    }

    pub fn update_static_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.static_cache).insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Static last-cache updated");
    }

    pub fn update_dynamic_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.dynamic_cache).insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Dynamic last-cache updated");
    }

    pub fn update_dynamic_summary_prebuilt(&self, uuid: Uuid, value: serde_json::Value) {
        recover_write(&self.dynamic_summary_cache).insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Dynamic-summary last-cache updated");
    }

    pub fn update_static(&self, uuid: Uuid, timestamp: i64, data: &StaticMonitoringData) {
        let value = build_static_value(uuid, timestamp, data);
        self.update_static_prebuilt(uuid, value);
    }

    pub fn update_dynamic(&self, uuid: Uuid, timestamp: i64, data: &DynamicMonitoringData) {
        let value = build_dynamic_value(uuid, timestamp, data);
        self.update_dynamic_prebuilt(uuid, value);
    }

    pub fn update_dynamic_summary(
        &self,
        uuid: Uuid,
        timestamp: i64,
        data: &DynamicMonitoringSummaryData,
    ) {
        let value = build_dynamic_summary_value(uuid, timestamp, data);
        self.update_dynamic_summary_prebuilt(uuid, value);
    }

    pub fn get_static_last(
        &self,
        uuid: &Uuid,
        fields: &[StaticDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.static_cache);
        let full_obj = guard.get(uuid)?.as_object()?;
        let mut filtered = serde_json::Map::with_capacity(fields.len() + 2);
        filtered.insert("uuid".to_owned(), full_obj.get("uuid")?.clone());
        filtered.insert("timestamp".to_owned(), full_obj.get("timestamp")?.clone());
        for field in fields {
            let key = field.json_key();
            if let Some(v) = full_obj.get(key) {
                filtered.insert(key.to_owned(), v.clone());
            }
        }
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Static last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }

    pub fn get_dynamic_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.dynamic_cache);
        let full_obj = guard.get(uuid)?.as_object()?;
        let mut filtered = serde_json::Map::with_capacity(fields.len() + 2);
        filtered.insert("uuid".to_owned(), full_obj.get("uuid")?.clone());
        filtered.insert("timestamp".to_owned(), full_obj.get("timestamp")?.clone());
        for field in fields {
            let key = field.json_key();
            if let Some(v) = full_obj.get(key) {
                filtered.insert(key.to_owned(), v.clone());
            }
        }
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }

    pub fn get_dynamic_summary_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicSummaryQueryField],
    ) -> Option<serde_json::Value> {
        let guard = recover_read(&self.dynamic_summary_cache);
        let full = guard.get(uuid)?;
        if fields.is_empty() {
            let cloned = full.clone();
            drop(guard);
            trace!(target: "monitoring", %uuid, field_count = 0, "Dynamic-summary last-cache hit (all fields)");
            return Some(cloned);
        }
        let full_obj = full.as_object()?;
        let mut filtered = serde_json::Map::with_capacity(fields.len() + 2);
        filtered.insert("uuid".to_owned(), full_obj.get("uuid")?.clone());
        filtered.insert("timestamp".to_owned(), full_obj.get("timestamp")?.clone());
        for field in fields {
            let key = field.json_key();
            if let Some(v) = full_obj.get(key) {
                filtered.insert(key.to_owned(), v.clone());
            }
        }
        drop(guard);
        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic-summary last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }
}

fn recover_read<K, V>(
    lock: &RwLock<HashMap<K, V>>,
) -> std::sync::RwLockReadGuard<'_, HashMap<K, V>> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

fn recover_write<K, V>(
    lock: &RwLock<HashMap<K, V>>,
) -> std::sync::RwLockWriteGuard<'_, HashMap<K, V>> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

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
