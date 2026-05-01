//! In-memory cache for the **latest** monitoring data per agent UUID.
//!
//! When an Agent reports static / dynamic / dynamic-summary data, the payload
//! is stored here (in addition to being buffered for DB insert).  
//! `*_multi_last_query` RPCs read from this cache **instead of hitting the
//! database**, making last-record lookups O(1) and zero DB load.
//!
//! The cache stores `serde_json::Value::Object` in the exact shape returned
//! by the database queries (`uuid`, `timestamp`, `cpu` / `cpu_data` renamed,
//! etc.), so the query handlers can treat cache hits and DB rows identically.

use nodeget_lib::monitoring::data_structure::{
    DynamicMonitoringData, DynamicMonitoringSummaryData, StaticMonitoringData,
};
use nodeget_lib::monitoring::query::{
    DynamicDataQueryField, DynamicSummaryQueryField, StaticDataQueryField,
};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, trace};
use uuid::Uuid;

// ── 全局单例 ──────────────────────────────────────────────────────────

static CACHE: OnceLock<MonitoringLastCache> = OnceLock::new();

pub struct MonitoringLastCache {
    static_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
    dynamic_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
    dynamic_summary_cache: RwLock<HashMap<Uuid, serde_json::Value>>,
}

impl MonitoringLastCache {
    /// Initialize the global cache (empty).
    pub fn init() {
        CACHE.get_or_init(|| Self {
            static_cache: RwLock::new(HashMap::new()),
            dynamic_cache: RwLock::new(HashMap::new()),
            dynamic_summary_cache: RwLock::new(HashMap::new()),
        });
    }

    /// Get the global instance.
    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("MonitoringLastCache not initialized — call MonitoringLastCache::init() first")
    }

    // ── Update helpers (called from report_* handlers) ──────────────

    /// Store the latest static monitoring data for `uuid`.
    pub async fn update_static(&self, uuid: Uuid, timestamp: i64, data: &StaticMonitoringData) {
        let mut obj = serde_json::Map::with_capacity(5);
        obj.insert("uuid".to_owned(), serde_json::Value::String(uuid.to_string()));
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

        let value = serde_json::Value::Object(obj);
        let mut guard = self.static_cache.write().await;
        guard.insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Static last-cache updated");
    }

    /// Store the latest dynamic monitoring data for `uuid`.
    pub async fn update_dynamic(&self, uuid: Uuid, timestamp: i64, data: &DynamicMonitoringData) {
        let mut obj = serde_json::Map::with_capacity(9);
        obj.insert("uuid".to_owned(), serde_json::Value::String(uuid.to_string()));
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

        let value = serde_json::Value::Object(obj);
        let mut guard = self.dynamic_cache.write().await;
        guard.insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Dynamic last-cache updated");
    }

    /// Store the latest dynamic summary data for `uuid`.
    pub async fn update_dynamic_summary(
        &self,
        uuid: Uuid,
        timestamp: i64,
        data: &DynamicMonitoringSummaryData,
    ) {
        let mut obj = serde_json::Map::with_capacity(24);
        obj.insert("uuid".to_owned(), serde_json::Value::String(uuid.to_string()));
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

        let value = serde_json::Value::Object(obj);
        let mut guard = self.dynamic_summary_cache.write().await;
        guard.insert(uuid, value);
        debug!(target: "monitoring", %uuid, "Dynamic-summary last-cache updated");
    }

    // ── Read helpers (called from query_*_multi_last handlers) ───────

    /// Try to read the last static record for `uuid`.
    ///
    /// `fields` controls which data columns are included (`[]` means all).
    /// Returns `None` if the UUID has not been reported yet.
    pub async fn get_static_last(
        &self,
        uuid: &Uuid,
        fields: &[StaticDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = self.static_cache.read().await;
        let full = guard.get(uuid)?.clone();

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

        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Static last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }

    /// Try to read the last dynamic record for `uuid`.
    ///
    /// `fields` controls which data columns are included (`[]` means all).
    /// Returns `None` if the UUID has not been reported yet.
    pub async fn get_dynamic_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicDataQueryField],
    ) -> Option<serde_json::Value> {
        let guard = self.dynamic_cache.read().await;
        let full = guard.get(uuid)?.clone();

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

        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }

    /// Try to read the last dynamic-summary record for `uuid`.
    ///
    /// `fields` controls which data columns are included (`[]` means all).
    /// Returns `None` if the UUID has not been reported yet.
    pub async fn get_dynamic_summary_last(
        &self,
        uuid: &Uuid,
        fields: &[DynamicSummaryQueryField],
    ) -> Option<serde_json::Value> {
        let guard = self.dynamic_summary_cache.read().await;
        let full = guard.get(uuid)?.clone();

        // When no fields are specified, return the full object to match the DB
        // path behavior (query_dynamic_summary.rs selects all columns when fields.is_empty()).
        if fields.is_empty() {
            return Some(full);
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

        trace!(target: "monitoring", %uuid, field_count = fields.len(), "Dynamic-summary last-cache hit");
        Some(serde_json::Value::Object(filtered))
    }
}
