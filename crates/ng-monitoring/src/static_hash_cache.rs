use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

struct Inner {
    by_uuid_id: HashMap<i16, Vec<u8>>,
}

pub struct StaticHashCache {
    inner: RwLock<Inner>,
}

static CACHE: OnceLock<StaticHashCache> = OnceLock::new();

fn recover_read(lock: &RwLock<Inner>) -> std::sync::RwLockReadGuard<'_, Inner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "static_hash_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

fn recover_write(lock: &RwLock<Inner>) -> std::sync::RwLockWriteGuard<'_, Inner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "static_hash_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

impl StaticHashCache {
    pub fn init() {
        CACHE.get_or_init(|| Self {
            inner: RwLock::new(Inner {
                by_uuid_id: HashMap::with_capacity(32),
            }),
        });
    }

    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("StaticHashCache not initialized — call StaticHashCache::init() first")
    }

    pub fn is_duplicate(&self, uuid_id: i16, data_hash: &[u8]) -> bool {
        let guard = recover_read(&self.inner);
        guard
            .by_uuid_id
            .get(&uuid_id)
            .is_some_and(|cached| cached == data_hash)
    }

    pub fn update(&self, uuid_id: i16, data_hash: Vec<u8>) {
        let mut guard = recover_write(&self.inner);
        guard.by_uuid_id.insert(uuid_id, data_hash);
    }
}
