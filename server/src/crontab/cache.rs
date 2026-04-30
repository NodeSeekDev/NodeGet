use crate::DB;
use crate::entity::crontab;
use cron::Schedule;
use nodeget_lib::crontab::CronType;
use nodeget_lib::error::NodegetError;
use sea_orm::EntityTrait;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Pre-parsed crontab entry: model + parsed Schedule + parsed `CronType`.
/// Avoids re-parsing `cron_expression` and `cron_type` on every tick.
pub struct CachedCrontab {
    pub model: Arc<crontab::Model>,
    pub schedule: Schedule,
    pub cron_type: CronType,
}

struct CrontabCacheInner {
    /// id -> pre-parsed entry
    by_id: HashMap<i64, CachedCrontab>,
}

pub struct CrontabCache {
    inner: RwLock<CrontabCacheInner>,
}

static CACHE: OnceLock<CrontabCache> = OnceLock::new();

impl CrontabCache {
    /// Initialize the global crontab cache by loading all entries from DB.
    /// Must be called after DB is initialized.
    pub async fn init() -> anyhow::Result<()> {
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all = crontab::Entity::find()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to load crontab: {e}")))?;

        let by_id = Self::build_cache(all);
        let count = by_id.len();
        let cache = Self {
            inner: RwLock::new(CrontabCacheInner { by_id }),
        };

        if CACHE.set(cache).is_err() {
            warn!(target: "crontab", "CrontabCache already initialized, reloading");
            Self::reload().await?;
        } else {
            info!(target: "crontab", count, "CrontabCache initialized");
        }

        Ok(())
    }

    /// Get the global cache instance.
    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("CrontabCache not initialized — call CrontabCache::init() first")
    }

    /// Reload all entries from DB into cache.
    /// Called after any CUD operation on the crontab table.
    pub async fn reload() -> anyhow::Result<()> {
        let Some(cache) = CACHE.get() else {
            return Ok(());
        };
        let db = DB.get().ok_or_else(|| {
            NodegetError::ConfigNotFound("Database connection not initialized".to_owned())
        })?;

        let all = crontab::Entity::find()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(format!("Failed to reload crontab: {e}")))?;

        let by_id = Self::build_cache(all);
        let mut guard = cache.inner.write().await;
        guard.by_id = by_id;
        drop(guard);

        debug!(target: "crontab", "CrontabCache reloaded");
        Ok(())
    }

    /// Get all enabled crontab entries with pre-parsed Schedule and `CronType`.
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

    /// Update `last_run_time` for a specific crontab entry in cache only.
    /// The DB update is done separately by the caller.
    pub async fn update_last_run_time(&self, id: i64, timestamp: i64) {
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.by_id.get_mut(&id) {
            let mut updated = (*entry.model).clone();
            updated.last_run_time = Some(timestamp);
            entry.model = Arc::new(updated);
        }
    }

    /// Build the cache map from a list of models.
    /// Parses Schedule and `CronType` for each entry; skips entries with invalid data.
    fn build_cache(models: Vec<crontab::Model>) -> HashMap<i64, CachedCrontab> {
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
}
