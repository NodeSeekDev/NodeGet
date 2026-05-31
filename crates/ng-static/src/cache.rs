use ng_db::entity::static_file as static_entity;
use ng_infra::make_global_cache;
use ng_infra::server::{DbBackedCache, load_from_db};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use tracing::warn;

pub struct CachedStatic {
    pub model: Arc<static_entity::Model>,
}

struct StaticCacheInner {
    by_name: HashMap<String, CachedStatic>,
    http_root_name: Option<String>,
}

pub struct StaticCache {
    inner: RwLock<StaticCacheInner>,
}

fn recover_read(
    lock: &RwLock<StaticCacheInner>,
) -> std::sync::RwLockReadGuard<'_, StaticCacheInner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "static_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

fn recover_write(
    lock: &RwLock<StaticCacheInner>,
) -> std::sync::RwLockWriteGuard<'_, StaticCacheInner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "static_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

make_global_cache!(StaticCache, STATIC_CACHE_GLOBAL);

impl DbBackedCache for StaticCache {
    type Model = static_entity::Model;

    fn cache_name() -> &'static str {
        "static"
    }

    fn build_cache(models: Vec<Self::Model>) -> Self {
        let (by_name, http_root_name) = Self::build_maps(models);
        Self {
            inner: RwLock::new(StaticCacheInner {
                by_name,
                http_root_name,
            }),
        }
    }

    #[allow(clippy::unused_async)]
    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let (by_name, http_root_name) = Self::build_maps(models);
        let mut guard = recover_write(&self.inner);
        guard.by_name = by_name;
        guard.http_root_name = http_root_name;
        drop(guard);
    }

    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<static_entity::Entity>()
    }
}

impl StaticCache {
    fn build_maps(
        models: Vec<static_entity::Model>,
    ) -> (HashMap<String, CachedStatic>, Option<String>) {
        let mut by_name = HashMap::with_capacity(models.len());
        let mut http_root_name: Option<String> = None;

        for model in models {
            if model.is_http_root {
                if http_root_name.is_none() {
                    http_root_name = Some(model.name.clone());
                } else {
                    warn!(
                        target: "static",
                        name = %model.name,
                        existing = %http_root_name.as_ref().unwrap_or(&"unknown".to_string()),
                        "duplicate is_http_root detected, ignoring"
                    );
                }
            }
            let name = model.name.clone();
            by_name.insert(
                name,
                CachedStatic {
                    model: Arc::new(model),
                },
            );
        }

        (by_name, http_root_name)
    }

    pub fn get_by_name(&self, name: &str) -> Option<Arc<static_entity::Model>> {
        recover_read(&self.inner)
            .by_name
            .get(name)
            .map(|c| Arc::clone(&c.model))
    }

    pub fn get_http_root(&self) -> Option<Arc<static_entity::Model>> {
        let guard = recover_read(&self.inner);
        let name = guard.http_root_name.as_ref()?;
        guard.by_name.get(name).map(|c| Arc::clone(&c.model))
    }

    pub fn get_all(&self) -> Vec<Arc<static_entity::Model>> {
        recover_read(&self.inner)
            .by_name
            .values()
            .map(|c| Arc::clone(&c.model))
            .collect()
    }

    pub fn exists(&self, name: &str) -> bool {
        recover_read(&self.inner).by_name.contains_key(name)
    }
}
