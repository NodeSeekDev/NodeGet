//! 静态监控数据哈希去重缓存。
//!
//! 维护每个设备（`uuid_id`）最近一次静态监控数据的 SHA-256 哈希值，
//! 用于 `report_static` 的快速去重路径：若哈希相同则跳过写入，
//! 避免重复的静态硬件信息占用存储空间。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

/// 缓存内部数据，按 `uuid_id` 存储最近一次的数据哈希。
struct Inner {
    /// `uuid_id` → 数据哈希（前 16 字节，栈上固定大小，避免每条目堆分配）
    by_uuid_id: HashMap<i16, [u8; 16]>,
}

/// 静态数据哈希去重缓存。
pub struct StaticHashCache {
    inner: RwLock<Inner>,
}

/// 全局 `StaticHashCache` 单例。
static CACHE: OnceLock<StaticHashCache> = OnceLock::new();

/// 从 `RwLock` 获取读锁，锁中毒时自动恢复。
fn recover_read(lock: &RwLock<Inner>) -> std::sync::RwLockReadGuard<'_, Inner> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!(target: "static_hash_cache", "lock poisoned during read, recovering");
        e.into_inner()
    })
}

/// 从 `RwLock` 获取写锁，锁中毒时自动恢复。
fn recover_write(lock: &RwLock<Inner>) -> std::sync::RwLockWriteGuard<'_, Inner> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!(target: "static_hash_cache", "lock poisoned during write, recovering");
        e.into_inner()
    })
}

/// 将字节切片截取为 16 字节数组，不足 16 字节补零。
fn truncate_to_16(bytes: &[u8]) -> [u8; 16] {
    let mut arr = [0u8; 16];
    let len = bytes.len().min(16);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

impl StaticHashCache {
    /// 初始化全局单例。
    pub fn init() {
        CACHE.get_or_init(|| Self {
            inner: RwLock::new(Inner {
                by_uuid_id: HashMap::with_capacity(32),
            }),
        });
    }

    /// 获取全局单例引用，未初始化时 panic。
    ///
    /// # Panics
    ///
    /// 若全局 `StaticHashCache` 未初始化（即未调用 `init()`）则 panic。
    pub fn global() -> &'static Self {
        CACHE
            .get()
            .expect("StaticHashCache not initialized — call StaticHashCache::init() first")
    }

    /// 判断指定设备的静态数据哈希是否与缓存中的相同（即数据重复）。
    ///
    /// - `uuid_id` — 设备数字 ID
    /// - `data_hash` — 新数据的哈希值
    /// - 返回值 — `true` 表示与缓存中已有的哈希相同，为重复数据
    pub fn is_duplicate(&self, uuid_id: i16, data_hash: &[u8]) -> bool {
        let guard = recover_read(&self.inner);
        guard
            .by_uuid_id
            .get(&uuid_id)
            .is_some_and(|cached| *cached == truncate_to_16(data_hash))
    }

    /// 更新指定设备的静态数据哈希缓存。
    ///
    /// - `uuid_id` — 设备数字 ID
    /// - `data_hash` — 新数据的哈希值
    pub fn update(&self, uuid_id: i16, data_hash: Vec<u8>) {
        let mut guard = recover_write(&self.inner);
        guard.by_uuid_id.insert(uuid_id, truncate_to_16(&data_hash));
    }
}
