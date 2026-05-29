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
use crate::cache::{DbBackedCache, load_from_db};
use crate::entity::monitoring_uuid;
use crate::make_global_cache;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

struct MonitoringUuidCacheInner {
    /// `uuid` → `(id, soft_delete)`
    by_uuid: HashMap<Uuid, (i16, bool)>,
    /// `id` → `(uuid, soft_delete)`
    by_id: HashMap<i16, (Uuid, bool)>,
}

pub struct MonitoringUuidCache {
    inner: RwLock<MonitoringUuidCacheInner>,
}

make_global_cache!(MonitoringUuidCache, MONITORING_UUID_CACHE_GLOBAL);

impl DbBackedCache for MonitoringUuidCache {
    type Model = monitoring_uuid::Model;

    fn cache_name() -> &'static str {
        "monitoring_uuid"
    }

    fn build_cache(models: Vec<Self::Model>) -> Self {
        let mut by_uuid = HashMap::with_capacity(models.len());
        let mut by_id = HashMap::with_capacity(models.len());
        for model in models {
            let id = model.id as i16;
            by_uuid.insert(model.uuid, (id, model.soft_delete));
            by_id.insert(id, (model.uuid, model.soft_delete));
        }
        Self {
            inner: RwLock::new(MonitoringUuidCacheInner { by_uuid, by_id }),
        }
    }

    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let mut by_uuid = HashMap::with_capacity(models.len());
        let mut by_id = HashMap::with_capacity(models.len());
        for model in models {
            let id = model.id as i16;
            by_uuid.insert(model.uuid, (id, model.soft_delete));
            by_id.insert(id, (model.uuid, model.soft_delete));
        }
        let mut guard = self.inner.write().await;
        guard.by_uuid = by_uuid;
        guard.by_id = by_id;
        drop(guard);
    }

    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<monitoring_uuid::Entity>()
    }
}

impl MonitoringUuidCache {
    /// Get the `id` for a `uuid` regardless of soft-delete state.
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

    /// Get or insert a `uuid` into the `monitoring_uuid` table.
    pub async fn get_or_insert(&self, uuid: Uuid) -> Result<i16, NodegetError> {
        // Fast path — read lock only
        {
            let guard = self.inner.read().await;
            if let Some((id, soft_delete)) = guard.by_uuid.get(&uuid) {
                if !soft_delete {
                    return Ok(*id);
                }
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
