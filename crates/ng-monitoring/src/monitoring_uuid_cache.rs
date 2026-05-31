use ng_core::error::NodegetError;
use ng_db::entity::monitoring_uuid;
use ng_infra::server::{DbBackedCache, load_from_db};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use std::future::Future;
use std::sync::RwLock;
use tracing::info;
use uuid::Uuid;

struct MonitoringUuidCacheInner {
    by_uuid: HashMap<Uuid, (i16, bool)>,
    by_id: HashMap<i16, (Uuid, bool)>,
}

pub struct MonitoringUuidCache {
    inner: RwLock<MonitoringUuidCacheInner>,
}

fn recover_read(
    lock: &RwLock<MonitoringUuidCacheInner>,
) -> std::sync::RwLockReadGuard<'_, MonitoringUuidCacheInner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring_uuid_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

fn recover_write(
    lock: &RwLock<MonitoringUuidCacheInner>,
) -> std::sync::RwLockWriteGuard<'_, MonitoringUuidCacheInner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "monitoring_uuid_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

ng_infra::make_global_cache!(MonitoringUuidCache, MONITORING_UUID_CACHE_GLOBAL);

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

    #[allow(clippy::unused_async)]
    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let mut by_uuid = HashMap::with_capacity(models.len());
        let mut by_id = HashMap::with_capacity(models.len());
        for model in models {
            let id = model.id as i16;
            by_uuid.insert(model.uuid, (id, model.soft_delete));
            by_id.insert(id, (model.uuid, model.soft_delete));
        }
        let mut guard = recover_write(&self.inner);
        guard.by_uuid = by_uuid;
        guard.by_id = by_id;
        drop(guard);
    }

    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<monitoring_uuid::Entity>()
    }
}

impl MonitoringUuidCache {
    pub fn get_id(&self, uuid: &Uuid) -> Option<i16> {
        recover_read(&self.inner)
            .by_uuid
            .get(uuid)
            .map(|(id, _)| *id)
    }

    pub fn get_uuid(&self, id: i16) -> Option<Uuid> {
        recover_read(&self.inner)
            .by_id
            .get(&id)
            .map(|(uuid, _)| *uuid)
    }

    pub fn is_active(&self, uuid: &Uuid) -> bool {
        recover_read(&self.inner)
            .by_uuid
            .get(uuid)
            .is_some_and(|(_, soft_delete)| !soft_delete)
    }

    pub fn exists(&self, uuid: &Uuid) -> bool {
        recover_read(&self.inner).by_uuid.contains_key(uuid)
    }

    pub fn list_all(&self) -> Vec<Uuid> {
        let guard = recover_read(&self.inner);
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

    pub fn list_all_with_agent_mode(&self) -> Vec<(Uuid, bool)> {
        let guard = recover_read(&self.inner);
        let mut result: Vec<(Uuid, bool)> = guard
            .by_uuid
            .iter()
            .map(|(uuid, (_, soft_delete))| (*uuid, *soft_delete))
            .collect();
        drop(guard);
        result.sort_by_key(|a| a.0);
        result
    }

    pub async fn get_or_insert(&self, uuid: Uuid) -> Result<i16, NodegetError> {
        {
            let guard = recover_read(&self.inner);
            if let Some((id, soft_delete)) = guard.by_uuid.get(&uuid)
                && !soft_delete
            {
                return Ok(*id);
            }
        }

        let db = ng_db::get_db().ok_or_else(|| {
            NodegetError::DatabaseError("Database connection not initialized".to_owned())
        })?;

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
            let mut guard = recover_write(&self.inner);
            guard.by_uuid.insert(uuid, (id, false));
            guard.by_id.insert(id, (uuid, false));
            return Ok(id);
        }

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
        let mut guard = recover_write(&self.inner);
        guard.by_uuid.insert(uuid, (id, false));
        guard.by_id.insert(id, (uuid, false));
        Ok(id)
    }

    pub async fn soft_delete(&self, uuid: Uuid) -> Result<bool, NodegetError> {
        let db = ng_db::get_db().ok_or_else(|| {
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
            return Ok(true);
        }

        let id = model.id as i16;
        let mut active: monitoring_uuid::ActiveModel = model.into();
        active.soft_delete = Set(true);
        active.update(db).await.map_err(|e| {
            NodegetError::DatabaseError(format!("Failed to soft_delete monitoring_uuid: {e}"))
        })?;

        let mut guard = recover_write(&self.inner);
        guard.by_uuid.insert(uuid, (id, true));
        guard.by_id.insert(id, (uuid, true));

        info!(target: "monitoring_uuid_cache", %uuid, "Soft-deleted uuid");
        Ok(true)
    }
}
