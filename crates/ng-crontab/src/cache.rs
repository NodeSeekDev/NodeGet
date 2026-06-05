//! Crontab 内存缓存：基于 DB 的全量加载缓存，供调度循环与 RPC 查询使用。
//!
//! 使用 `DbBackedCache` trait + `make_global_cache!` 宏生成全局单例，
//! 提供 `init()` / `global()` / `reload()` 方法。
//! 调度器每分钟读取缓存中已启用的条目，判断是否到期触发。

use crate::CronType;
use cron::Schedule;
use ng_db::entity::crontab;
use ng_infra::make_global_cache;
use ng_infra::server::{DbBackedCache, load_from_db};
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tracing::warn;

/// 缓存中的单条定时任务条目，预解析了 cron 表达式和类型。
pub struct CachedCrontab {
    /// 数据库行模型，外层 Arc<CachedCrontab> 已提供共享，无需内层 Arc
    pub model: crontab::Model,
    /// 预编译的 cron 调度对象，用于判断下次触发时间
    pub schedule: Schedule,
    /// 预解析的定时任务类型（Agent / Server）
    pub cron_type: CronType,
}

/// CrontabCache 内部状态，由 RwLock 保护以支持并发读写。
struct CrontabCacheInner {
    /// 按 ID 索引的定时任务缓存
    by_id: HashMap<i64, Arc<CachedCrontab>>,
    /// 单独追踪 last_run_time，避免为更新时间戳而深拷贝整个 CachedCrontab。
    /// 键为 crontab id，值为毫秒时间戳。优先于 model.last_run_time 生效。
    last_run_times: RwLock<HashMap<i64, i64>>,
}

/// 基于 DB 的 Crontab 全量缓存，提供按 ID 查询、启用条目枚举、
/// last_run_time 原子更新等操作。
pub struct CrontabCache {
    inner: RwLock<CrontabCacheInner>,
}

/// 从 RwLock 获取读锁，若锁被 poisoned 则恢复并继续（而非 panic）。
fn recover_read(
    lock: &RwLock<CrontabCacheInner>,
) -> std::sync::RwLockReadGuard<'_, CrontabCacheInner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "crontab_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

/// 从 RwLock 获取写锁，若锁被 poisoned 则恢复并继续（而非 panic）。
fn recover_write(
    lock: &RwLock<CrontabCacheInner>,
) -> std::sync::RwLockWriteGuard<'_, CrontabCacheInner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "crontab_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

// 生成全局单例：CRONTAB_CACHE_GLOBAL，提供 init() / global() / reload()
make_global_cache!(CrontabCache, CRONTAB_CACHE_GLOBAL);

impl DbBackedCache for CrontabCache {
    type Model = crontab::Model;

    /// 缓存名称标识，用于日志输出。
    fn cache_name() -> &'static str {
        "crontab"
    }

    /// 从数据库模型列表构建缓存实例。
    fn build_cache(models: Vec<Self::Model>) -> Self {
        let by_id = Self::build_maps(models);
        Self {
            inner: RwLock::new(CrontabCacheInner {
                by_id,
                last_run_times: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// 热重载：用新模型替换 by_id，保留 last_run_times 覆盖映射。
    /// last_run_times 在 reload 期间不丢失，保证调度器的时间戳追踪连续。
    #[allow(clippy::unused_async)]
    async fn reload_from_models(&self, models: Vec<Self::Model>) {
        let by_id = Self::build_maps(models);
        let mut guard = recover_write(&self.inner);
        guard.by_id = by_id;
        drop(guard);
    }

    /// 从数据库全量加载 crontab 表。
    fn load_all() -> impl Future<Output = anyhow::Result<Vec<Self::Model>>> + Send {
        load_from_db::<crontab::Entity>()
    }
}

impl CrontabCache {
    /// 从数据库模型列表构建 ID 索引映射。
    /// 解析失败的条目（无效 cron 表达式或无效 cron_type）会被跳过并记录警告日志。
    fn build_maps(models: Vec<crontab::Model>) -> HashMap<i64, Arc<CachedCrontab>> {
        let mut by_id = HashMap::with_capacity(models.len());
        for mut model in models {
            // 解析 cron 表达式为 Schedule 对象
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

            // 取走 cron_type 所有权直接解析，避免 clone 整个 Value
            // 缓存中通过 CachedCrontab.cron_type 访问，model.cron_type 不再被读取
            let cron_type = match serde_json::from_value::<CronType>(std::mem::take(&mut model.cron_type)) {
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
                Arc::new(CachedCrontab {
                    model,
                    schedule,
                    cron_type,
                }),
            );
        }
        by_id
    }

    /// 获取所有已启用的定时任务条目，供调度循环使用。
    pub fn get_enabled_entries(&self) -> Vec<Arc<CachedCrontab>> {
        let guard = recover_read(&self.inner);
        guard
            .by_id
            .values()
            .filter(|entry| entry.model.enable)
            .map(Arc::clone)
            .collect()
    }

    /// 获取全部缓存条目（含已禁用的），供 `get` RPC 使用以避免查询数据库。
    pub fn get_all_entries(&self) -> Vec<Arc<CachedCrontab>> {
        let guard = recover_read(&self.inner);
        guard.by_id.values().map(Arc::clone).collect()
    }

    /// 获取指定定时任务的有效 last_run_time。
    /// 优先从覆盖映射中读取（调度器实时更新），若不存在则回退到 model.last_run_time。
    ///
    /// - `id` - 定时任务 ID
    /// - `model_last` - 数据库模型中的 last_run_time（毫秒时间戳）
    pub fn get_last_run_time(&self, id: i64, model_last: Option<i64>) -> Option<i64> {
        let guard = recover_read(&self.inner);
        guard
            .last_run_times
            .read()
            .unwrap_or_else(|e| {
                warn!(target: "crontab_cache", "last_run_times lock poisoned during read, recovering");
                e.into_inner()
            })
            .get(&id)
            .copied()
            .or(model_last)
    }

    /// 更新指定定时任务的 last_run_time 到覆盖映射。
    /// 由调度器在触发任务后立即调用，保证下次调度时不会重复触发。
    ///
    /// - `id` - 定时任务 ID
    /// - `timestamp` - 当前触发时间（毫秒时间戳）
    pub fn update_last_run_time(&self, id: i64, timestamp: i64) {
        let guard = recover_read(&self.inner);
        let mut lrt_guard = guard.last_run_times.write().unwrap_or_else(|e| {
            warn!(target: "crontab_cache", "last_run_times lock poisoned during write, recovering");
            e.into_inner()
        });
        lrt_guard.insert(id, timestamp);
    }
}
