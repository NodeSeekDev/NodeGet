use crate::cache::{DbBackedCache, load_from_db};
use crate::entity::crontab;
use crate::make_global_cache;
use cron::Schedule;
use nodeget_lib::crontab::CronType;
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

/// Pre-parsed crontab entry: model + parsed Schedule + parsed `CronType`.
pub struct CachedCrontab {
    pub model: Arc<crontab::Model>,
    pub schedule: Schedule,
    pub cron_type: CronType,
}

struct CrontabCacheInner {
    by_id: HashMap<i64, CachedCrontab>,
}

pub struct CrontabCache {
    inner: RwLock<CrontabCacheInner>,
}

make_global_cache!(CrontabCache, CRONTAB_CACHE_GLOBAL);

impl DbBackedCache for CrontabCache {
    type Model = crontab::Model;

    fn cache_name() -> &'static str {
        "crontab"
    }

    fn build_cache(models: Vec<Self::Model>) -> Self {
        let by_id = Self::build_maps(models);
        Self {
            inner: RwLock::new(CrontabCacheInner { by_id }),
        }
    }

    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let by_id = Self::build_maps(models);
        let mut guard = self.inner.write().await;
        guard.by_id = by_id;
        drop(guard);
    }

    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<crontab::Entity>()
    }
}

impl CrontabCache {
    fn build_maps(models: Vec<crontab::Model>) -> HashMap<i64, CachedCrontab> {
        let mut by_id = HashMap::with_capacity(models.len());
        for model in models {
            let schedule = match Schedule::from_str(&model.cron_expression) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        target: "crontab",
                        job_id = model.id,
                        job_name = %model.name,
                        error = %e,
                        "invalid cron expression during cache build, skipping"
                    );
                    continue;
                }
            };

            let cron_type = match serde_json::from_value::<CronType>(model.cron_type.clone()) {
                Ok(ct) => ct,
                Err(e) => {
                    warn!(
                        target: "crontab",
                        job_id = model.id,
                        job_name = %model.name,
                        error = %e,
                        "invalid cron_type during cache build, skipping"
                    );
                    continue;
                }
            };

            let id = model.id;
            by_id.insert(
                id,
                CachedCrontab {
                    model: Arc::new(model),
                    schedule,
                    cron_type,
                },
            );
        }
        by_id
    }

    pub async fn get_enabled_entries(&self) -> Vec<(Arc<crontab::Model>, Schedule, CronType)> {
        let guard = self.inner.read().await;
        guard
            .by_id
            .values()
            .filter(|entry| entry.model.enable)
            .map(|entry| {
                (
                    Arc::clone(&entry.model),
                    entry.schedule.clone(),
                    entry.cron_type.clone(),
                )
            })
            .collect()
    }

    pub async fn update_last_run_time(&self, id: i64, timestamp: i64) {
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.by_id.get_mut(&id) {
            let mut updated = (*entry.model).clone();
            updated.last_run_time = Some(timestamp);
            entry.model = Arc::new(updated);
        }
    }
}
