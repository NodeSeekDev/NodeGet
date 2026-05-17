//! In-memory cache for the `monitoring_uuid` table.
//!
//! After the 2025-05 refactoring, `monitoring_uuid` is the **authoritative Agent table**.
//! All agent CRUD flows through this cache, which stays in sync with the DB:
//!
//! - `init()`   — loads the entire table into memory at startup.
//! - `reload()` — rebuilds from DB after any mutation.
//! - `list_all()` — returns only **non-soft-deleted** UUIDs (O(1) in-RAM).
//! - `get_or_insert()` — fetches existing id, or INSERTs a new row.
//!   If the row exists but `soft_delete = true`, it is **resurrected** automatically.
//! - `soft_delete()` — marks a row as soft-deleted.
//!
//! `read` operations hit RAM directly; `write` operations update the DB
//! and then call `reload()` to keep the cache consistent.

use crate::DB;
use crate::entity::monitoring_uuid;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── 全局单例 ──────────────────────────────────────────────────────────

static CACHE: OnceLock<MonitoringUuidCache> = OnceLock::new();

struct MonitoringUuidCacheInner {
    /// `uuid` → `(id, soft_delete)`
    by_uuid: HashMap<Uuid, (i16, bool)>,
    /// `id` → `(uuid, soft_delete)`
    by_id: HashMap<i16, (Uuid, bool)>,
}

pub struct MonitoringUuidCache {
    inner: RwLock<MonitoringUuidCacheInner>,
}

impl MonitoringUuidCache {
    /// Initialize the global cache by loading all rows from DB.
    pub async fn init() -> anyhow::Result<()> {
        let cache = Self {
            inner: RwLock::new(MonitoringUuidCacheInner {
                by_uuid: HashMap::new(),
                by_id: HashMap::new(),
            }),
        };

        if CACHE.set(cache).is_err() {
            warn!(target: "monitoring_uuid_cache", "MonitoringUuidCache already initialized, reloading");
            Self::reload().await?;
            return Ok(());
        }

        Self::reload().await?;
        info!(target: "monitoring_uuid_cache", "MonitoringUuidCache initialized");
        Ok(())
    }

    /// Get the global instance. Panics if `init()` has not been called.
    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("MonitoringUuidCache not initialized — call MonitoringUuidCache::init() first")
    }

    // ── Reload ──────────────────────────────────────────────────────────

    /// Rebuild the in-memory maps from the current DB state.
    pub async fn reload() -> anyhow::Result<()> {
        let Some(cache) = CACHE.get() else {
            return Ok(());
        };
        let db = DB.get().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        let all = monitoring_uuid::Entity::find().all(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to reload monitoring_uuid: {e}"))
        })?;

        let mut by_uuid = HashMap::with_capacity(all.len());
        let mut by_id = HashMap::with_capacity(all.len());

        for model in all {
            let id = model.id as i16;
            by_uuid.insert(model.uuid, (id, model.soft_delete));
            by_id.insert(id, (model.uuid, model.soft_delete));
        }

        let mut guard = cache.inner.write().await;
        guard.by_uuid = by_uuid;
        guard.by_id = by_id;
        drop(guard);

        debug!(target: "monitoring_uuid_cache", "MonitoringUuidCache reloaded");
        Ok(())
    }

    // ── Read helpers ────────────────────────────────────────────────────

    /// Get the `id` for a `uuid` regardless of soft-delete state.
    /// Returns `None` only if the uuid has never been seen.
    pub async fn get_id(&self, uuid: &Uuid) -> Option<i16> {
        let guard = self.inner.read().await;
        guard.by_uuid.get(uuid).map(|(id, _)| *id)
    }

    /// Get the `uuid` for an `id` regardless of soft-delete state.
    pub async fn get_uuid(&self, id: i16) -> Option<Uuid> {
        let guard = self.inner.read().await;
        guard.by_id.get(&id).map(|(uuid, _)| *uuid)
    }

    /// Returns `true` if the uuid exists and is **not** soft-deleted.
    pub async fn is_active(&self, uuid: &Uuid) -> bool {
        let guard = self.inner.read().await;
        guard
            .by_uuid
            .get(uuid)
            .is_some_and(|(_, soft_delete)| !soft_delete)
    }

    /// Returns `true` if the uuid exists in the table (in any state).
    pub async fn exists(&self, uuid: &Uuid) -> bool {
        let guard = self.inner.read().await;
        guard.by_uuid.contains_key(uuid)
    }

    /// List all **active** (non-soft-deleted) UUIDs, sorted for stable output.
    pub async fn list_all(&self) -> Vec<Uuid> {
        let guard = self.inner.read().await;
        let mut uuids: Vec<Uuid> = guard
            .by_uuid
            .iter()
            .filter(|(_, (_, soft_delete))| !soft_delete)
            .map(|(uuid, _)| *uuid)
            .collect();
        drop(guard);
        uuids.sort();
        uuids
    }

    /// List all UUIDs with their soft-delete status, sorted for stable output.
    pub async fn list_all_with_agent_mode(&self) -> Vec<(Uuid, bool)> {
        let guard = self.inner.read().await;
        let mut result: Vec<(Uuid, bool)> = guard
            .by_uuid
            .iter()
            .map(|(uuid, (_, soft_delete))| (*uuid, *soft_delete))
            .collect();
        drop(guard);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    // ── Write helpers ───────────────────────────────────────────────────

    /// Get or insert a `uuid` into the `monitoring_uuid` table.
    ///
    /// Behaviour:
    /// - If the uuid exists and is **active** → return the id.
    /// - If the uuid exists but is **soft-deleted** → resurrect it
    ///   (`UPDATE soft_delete = false`) and reload the cache.
    /// - If the uuid does **not** exist → INSERT a new row and reload.
    pub async fn get_or_insert(&self, uuid: Uuid) -> Result<i16, NodegetError> {
        // Fast path — read lock only
        {
            let guard = self.inner.read().await;
            if let Some((id, soft_delete)) = guard.by_uuid.get(&uuid) {
                if !soft_delete {
                    return Ok(*id);
                }
                // soft-deleted — fall through to resurrection
            }
        }

        let db = DB.get().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        // Check DB state (the cache might be stale)
        let existing = monitoring_uuid::Entity::find()
            .filter(monitoring_uuid::Column::Uuid.eq(uuid))
            .one(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to query monitoring_uuid: {e}"))
            })?;

        if let Some(model) = existing {
            let id = model.id as i16;
            if model.soft_delete {
                let mut active: monitoring_uuid::ActiveModel = model.into();
                active.soft_delete = Set(false);
                active.update(db).await.map_err(|e| {
                    NodegetError::DatabaseError(format!("Failed to resurrect monitoring_uuid: {e}"))
                })?;
                info!(target: "monitoring_uuid_cache", %uuid, "Resurrected soft-deleted uuid");
            }
            Self::reload().await.map_err(|e| {
                NodegetError::DatabaseError(format!(
                    "Failed to reload cache after get_or_insert: {e}"
                ))
            })?;
            return Ok(id);
        }

        // Insert new
        let new_model = monitoring_uuid::ActiveModel {
            id: ActiveValue::default(),
            uuid: Set(uuid),
            soft_delete: Set(false),
        };

        let result = monitoring_uuid::Entity::insert(new_model)
            .exec(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to insert monitoring_uuid: {e}"))
            })?;

        let id = result.last_insert_id as i16;
        Self::reload().await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to reload cache after insert: {e}"))
        })?;
        Ok(id)
    }

    /// Soft-delete a uuid.
    ///
    /// Returns `Ok(true)` if the row was found and marked deleted.
    /// Returns `Ok(false)` if the uuid does not exist.
    pub async fn soft_delete(&self, uuid: Uuid) -> Result<bool, NodegetError> {
        let db = DB.get().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

        let existing = monitoring_uuid::Entity::find()
            .filter(monitoring_uuid::Column::Uuid.eq(uuid))
            .one(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!(
                    "Failed to query monitoring_uuid for soft_delete: {e}"
                ))
            })?;

        let Some(model) = existing else {
            return Ok(false);
        };

        if model.soft_delete {
            return Ok(true); // already soft-deleted, idempotent
        }

        let mut active: monitoring_uuid::ActiveModel = model.into();
        active.soft_delete = Set(true);
        active.update(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to soft_delete monitoring_uuid: {e}"))
        })?;

        Self::reload().await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to reload cache after soft_delete: {e}"))
        })?;

        info!(target: "monitoring_uuid_cache", %uuid, "Soft-deleted uuid");
        Ok(true)
    }
}
