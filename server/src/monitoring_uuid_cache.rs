use crate::DB;
use crate::entity::monitoring_uuid;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

struct MonitoringUuidCacheInner {
    by_uuid: HashMap<Uuid, i16>,
    by_id: HashMap<i16, Uuid>,
}

pub struct MonitoringUuidCache {
    inner: RwLock<MonitoringUuidCacheInner>,
}

static CACHE: OnceLock<MonitoringUuidCache> = OnceLock::new();

impl MonitoringUuidCache {
    /// Initialize the global cache by loading all entries from DB.
    pub async fn init() -> anyhow::Result<()> {
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all = monitoring_uuid::Entity::find()
            .all(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to load monitoring_uuid: {e}"))
            })?;

        let mut by_uuid = HashMap::with_capacity(all.len());
        let mut by_id = HashMap::with_capacity(all.len());

        for model in &all {
            by_uuid.insert(model.uuid, model.id);
            by_id.insert(model.id, model.uuid);
        }

        let count = all.len();
        let cache = MonitoringUuidCache {
            inner: RwLock::new(MonitoringUuidCacheInner { by_uuid, by_id }),
        };

        if CACHE.set(cache).is_err() {
            warn!(target: "monitoring", "MonitoringUuidCache already initialized, reloading");
            Self::reload().await?;
        } else {
            info!(target: "monitoring", count, "MonitoringUuidCache initialized");
        }

        Ok(())
    }

    /// Get the global cache instance.
    pub fn global() -> &'static MonitoringUuidCache {
        CACHE
            .get()
            .expect("MonitoringUuidCache not initialized — call init() first")
    }

    /// Reload all entries from DB into cache.
    pub async fn reload() -> anyhow::Result<()> {
        let Some(cache) = CACHE.get() else {
            return Ok(());
        };
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all = monitoring_uuid::Entity::find()
            .all(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to reload monitoring_uuid: {e}"))
            })?;

        let mut by_uuid = HashMap::with_capacity(all.len());
        let mut by_id = HashMap::with_capacity(all.len());

        for model in &all {
            by_uuid.insert(model.uuid, model.id);
            by_id.insert(model.id, model.uuid);
        }

        let mut guard = cache.inner.write().await;
        guard.by_uuid = by_uuid;
        guard.by_id = by_id;

        debug!(target: "monitoring", "MonitoringUuidCache reloaded");
        Ok(())
    }

    /// Get the i16 id for a UUID. Returns None if not in cache.
    pub async fn get_id(&self, uuid: &Uuid) -> Option<i16> {
        let guard = self.inner.read().await;
        guard.by_uuid.get(uuid).copied()
    }

    /// Get the UUID for an i16 id. Returns None if not in cache.
    pub async fn get_uuid(&self, id: i16) -> Option<Uuid> {
        let guard = self.inner.read().await;
        guard.by_id.get(&id).copied()
    }

    /// Get or insert a UUID, returning its i16 id.
    /// If the UUID is already cached, returns immediately.
    /// Otherwise inserts into DB, updates cache, and returns the new id.
    pub async fn get_or_insert(&self, uuid: Uuid) -> anyhow::Result<i16> {
        // Fast path: read lock
        {
            let guard = self.inner.read().await;
            if let Some(&id) = guard.by_uuid.get(&uuid) {
                return Ok(id);
            }
        }

        // Slow path: insert into DB
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        // Try to find first (another thread may have inserted)
        let existing = monitoring_uuid::Entity::find()
            .filter(monitoring_uuid::Column::Uuid.eq(uuid))
            .one(db)
            .await
            .map_err(|e| {
                NodegetError::DatabaseError(format!("Failed to query monitoring_uuid: {e}"))
            })?;

        let id = if let Some(model) = existing {
            model.id
        } else {
            let new_model = monitoring_uuid::ActiveModel {
                id: ActiveValue::default(),
                uuid: Set(uuid),
            };
            match monitoring_uuid::Entity::insert(new_model).exec(db).await {
                Ok(result) => {
                    debug!(target: "monitoring", %uuid, id = result.last_insert_id, "New monitoring UUID registered");
                    result.last_insert_id
                }
                Err(_) => {
                    // UNIQUE constraint violation — another thread inserted concurrently
                    // Re-query to get the id
                    let model = monitoring_uuid::Entity::find()
                        .filter(monitoring_uuid::Column::Uuid.eq(uuid))
                        .one(db)
                        .await
                        .map_err(|e| {
                            NodegetError::DatabaseError(format!(
                                "Failed to re-query monitoring_uuid after conflict: {e}"
                            ))
                        })?
                        .ok_or_else(|| {
                            NodegetError::DatabaseError(
                                "monitoring_uuid row disappeared after insert conflict".to_owned(),
                            )
                        })?;
                    debug!(target: "monitoring", %uuid, id = model.id, "Monitoring UUID resolved after concurrent insert");
                    model.id
                }
            }
        };

        // Update cache
        {
            let mut guard = self.inner.write().await;
            guard.by_uuid.insert(uuid, id);
            guard.by_id.insert(id, uuid);
        }

        Ok(id)
    }

    /// Get all known UUIDs.
    pub async fn get_all_uuids(&self) -> Vec<Uuid> {
        let guard = self.inner.read().await;
        guard.by_uuid.keys().copied().collect()
    }
}
