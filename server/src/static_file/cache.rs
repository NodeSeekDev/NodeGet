use crate::cache::{DbBackedCache, load_from_db};
use crate::entity::static_file as static_entity;
use crate::make_global_cache;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;
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

    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let (by_name, http_root_name) = Self::build_maps(models);
        let mut guard = self.inner.write().await;
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
            by_name.insert(name, CachedStatic {
                model: Arc::new(model),
            });
        }

        (by_name, http_root_name)
    }

    pub async fn get_by_name(&self, name: &str) -> Option<Arc<static_entity::Model>> {
        let guard = self.inner.read().await;
        guard.by_name.get(name).map(|c| Arc::clone(&c.model))
    }

    pub async fn get_http_root(&self) -> Option<Arc<static_entity::Model>> {
        let guard = self.inner.read().await;
        let name = guard.http_root_name.as_ref()?;
        guard.by_name.get(name).map(|c| Arc::clone(&c.model))
    }

    pub async fn get_all(&self) -> Vec<Arc<static_entity::Model>> {
        let guard = self.inner.read().await;
        guard
            .by_name
            .values()
            .map(|c| Arc::clone(&c.model))
            .collect()
    }

    pub async fn exists(&self, name: &str) -> bool {
        let guard = self.inner.read().await;
        guard.by_name.contains_key(name)
    }
}
