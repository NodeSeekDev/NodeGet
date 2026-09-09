//! 监控数据批量写入缓冲区。
//!
//! 使用 mpsc channel + 后台 tokio task 实现异步批量写入。
//! Agent 上报的监控数据先送入 `channel`，`flush_loop` 按 tick 或数据量阈值
//! 触发 `insert_many` 批量写入数据库，避免逐条 INSERT 造成的性能瓶颈。
//!
//! 核心组件：
//! - `MonitoringBuffers` — 全局单例，持有三个 `BufferSender`（static/dynamic/summary）
//! - `BufferSender<T>` — 线程安全的 mpsc Sender 封装，满时丢弃并告警
//! - `flush_loop` — 后台定时 flush 循环，channel 关闭时执行最后一次 flush

use ng_config::config::server::MonitoringBufferConfig;
use ng_db::entity::{dynamic_monitoring, dynamic_monitoring_summary, static_monitoring};
use sea_orm::EntityTrait;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval};
use tracing::{debug, error, trace, warn};

// ── 默认值 ──────────────────────────────────────────────────────────

/// 默认 flush 间隔（毫秒）
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 500;
/// 默认单次批量写入最大行数
const DEFAULT_MAX_BATCH_SIZE: usize = 1000;
/// 默认 channel 容量
const DEFAULT_CHANNEL_CAPACITY: usize = 10000;

// ── 全局单例 ────────────────────────────────────────────────────────

/// 全局 `MonitoringBuffers` 单例。
///
/// 用 `Mutex<Option<…>>` 而非 `OnceLock`，使配置热重载（reload）时 `flush_and_shutdown`
/// 能 `take()` 走旧实例，下一次 `init()` 重新 `replace` 并 spawn 新的 flush_loop。
/// 若用 `OnceLock`（set 后不可重置），reload 后 `init()` 会提前 return 不重建 flush_loop，
/// 而旧 sender 已被 `flush_and_shutdown` 关闭，导致监控数据静默全丢。详见 REVIEW C-1。
static BUFFERS: std::sync::Mutex<Option<MonitoringBuffers>> = std::sync::Mutex::new(None);

/// 在已初始化的 buffer 上运行闭包；未初始化时返回 `None`。
///
/// 不返回 `&MonitoringBuffers` 引用，因为底层是 `Mutex`，guard 不能跨 `&'static` 暴露。
/// 调用方（report_static/dynamic/summary 的 send 路径）用闭包做短暂同步访问即可。
pub fn with_buffers<R>(f: impl FnOnce(&MonitoringBuffers) -> R) -> Option<R> {
    let guard = BUFFERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_ref().map(f)
}

/// 全局 `flush_loop` `JoinHandle`，`flush_and_shutdown` 通过这些 handle 等待 flush 完成。
static FLUSH_HANDLES: std::sync::Mutex<Option<[JoinHandle<()>; 3]>> = std::sync::Mutex::new(None);

/// 持有三类监控数据的 `BufferSender`。
pub struct MonitoringBuffers {
    /// 静态监控数据缓冲区
    pub static_mon: BufferSender<static_monitoring::ActiveModel>,
    /// 动态监控数据缓冲区
    pub dynamic_mon: BufferSender<dynamic_monitoring::ActiveModel>,
    /// 动态摘要监控数据缓冲区
    pub dynamic_summary: BufferSender<dynamic_monitoring_summary::ActiveModel>,
}

/// 初始化全局 buffer 并启动三个后台 flush task。
///
/// - `config` — 可选的缓冲区配置，未提供时使用默认值
/// - 1. 从配置中读取 flush 间隔、批量大小等参数
/// - 2. 创建三个 mpsc channel（static/dynamic/summary）
/// - 3. 设置全局单例（重复调用时跳过）
/// - 4. 为每个 channel 启动独立的 `flush_loop` tokio task
///
/// 内部 `FLUSH_HANDLES` Mutex 即便被 poison 也通过 `into_inner` 恢复，不会 panic。
pub fn init(config: Option<&MonitoringBufferConfig>) {
    let flush_interval_ms = config
        .and_then(|c| c.flush_interval_ms)
        .unwrap_or(DEFAULT_FLUSH_INTERVAL_MS);
    let max_batch_size = config
        .and_then(|c| c.max_batch_size)
        .unwrap_or(DEFAULT_MAX_BATCH_SIZE);
    let channel_capacity = config
        .and_then(|c| c.channel_capacity)
        .unwrap_or(DEFAULT_CHANNEL_CAPACITY);

    let flush_interval = Duration::from_millis(flush_interval_ms);

    let (static_tx, static_rx) = mpsc::channel(channel_capacity);
    let (dynamic_tx, dynamic_rx) = mpsc::channel(channel_capacity);
    let (summary_tx, summary_rx) = mpsc::channel(channel_capacity);

    let buffers = MonitoringBuffers {
        static_mon: BufferSender::new(static_tx, channel_capacity),
        dynamic_mon: BufferSender::new(dynamic_tx, channel_capacity),
        dynamic_summary: BufferSender::new(summary_tx, channel_capacity),
    };

    // 用新实例覆盖全局单例。reload 场景下 flush_and_shutdown 已 take() 走旧实例，
    // 此处 replace 会覆盖任何残留（drop 旧 sender），保证下次 send 走新 channel。
    // 注意：若旧 flush_loop 仍在跑（异常情况），其 channel 已被 replace 后的新 sender
    // 隔离，旧 receiver 收到 None 退出，不会与新 buffer 串话。
    *BUFFERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(buffers);

    // 启动三个后台 flush task，保存 JoinHandle 用于 shutdown 时等待
    // 各表列数：static_monitoring=8, dynamic_monitoring=11, dynamic_monitoring_summary=27
    // 用于计算 SQLite 子批次大小 (999 / num_columns)
    let handles = [
        tokio::spawn(flush_loop::<
            static_monitoring::Entity,
            static_monitoring::ActiveModel,
        >(
            "static_monitoring",
            static_rx,
            flush_interval,
            max_batch_size,
            8,
        )),
        tokio::spawn(flush_loop::<
            dynamic_monitoring::Entity,
            dynamic_monitoring::ActiveModel,
        >(
            "dynamic_monitoring",
            dynamic_rx,
            flush_interval,
            max_batch_size,
            11,
        )),
        tokio::spawn(flush_loop::<
            dynamic_monitoring_summary::Entity,
            dynamic_monitoring_summary::ActiveModel,
        >(
            "dynamic_monitoring_summary",
            summary_rx,
            flush_interval,
            max_batch_size,
            27,
        )),
    ];
    *FLUSH_HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handles);

    debug!(
        target: "monitoring",
        flush_interval_ms = flush_interval_ms,
        max_batch_size = max_batch_size,
        channel_capacity = channel_capacity,
        "Monitoring write buffers initialized"
    );
}

/// 刷新所有缓冲区并等待完成（用于 graceful shutdown 与配置热重载）。
///
/// Drop 掉所有 `sender` 使 `channel` 关闭，`flush_loop` 的 `rx.recv()` 返回 `None`，
/// 触发最后一次 flush 后退出。通过 `JoinHandle` + timeout 等待 flush 完成，避免固定 sleep。
///
/// 关键：本函数 **take() 走全局单例**（清空 `BUFFERS`），使下一次 `init()` 能重新
/// `replace` 并 spawn 新的 flush_loop。否则 `OnceLock`/残留单例会让 reload 后的 `init()`
/// 提前 return，监控数据静默全丢（详见 REVIEW C-1）。
///
/// 内部 `FLUSH_HANDLES` Mutex 即便被 poison 也通过 `into_inner` 恢复，不会 panic。
pub async fn flush_and_shutdown() {
    let Some(buffers) = BUFFERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    else {
        return;
    };
    // Drop 所有 sender，关闭 channel
    buffers.static_mon.close();
    buffers.dynamic_mon.close();
    buffers.dynamic_summary.close();

    // 通过 JoinHandle 并发等待所有 flush_loop 完成最终 flush，5 秒超时兜底
    // 使用 join_all 并发等待，总超时 5 秒（而非每个 handle 顺序等待各 5 秒）
    let handles_opt = FLUSH_HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(handles) = handles_opt {
        let timeout_dur = Duration::from_secs(5);
        let result =
            tokio::time::timeout(timeout_dur, futures_util::future::join_all(handles)).await;
        match result {
            Ok(results) => {
                for (i, res) in results.into_iter().enumerate() {
                    if let Err(e) = res {
                        warn!(target: "monitoring", handle_index = i, error = %e, "Flush loop task panicked");
                    }
                }
            }
            Err(_) => {
                warn!(target: "monitoring", "Not all flush loops exited within 5s timeout");
            }
        }
    }
    debug!(target: "monitoring", "Monitoring buffers shutdown complete");
}

// ── BufferSender ────────────────────────────────────────────────────

/// mpsc Sender 的线程安全封装，支持优雅关闭与丢弃计数。
pub struct BufferSender<T> {
    /// 内部 Sender，close 时 take 出来 drop 以关闭 channel
    tx: std::sync::Mutex<Option<mpsc::Sender<T>>>,
    /// channel 容量，用于日志
    cap: usize,
    /// 累计丢弃数量
    dropped: AtomicU64,
    /// 是否已关闭（close 时置 true，send 的 fast-path 无需加锁）
    closed: AtomicBool,
}

impl<T> BufferSender<T> {
    /// 创建新的 `BufferSender`。
    const fn new(tx: mpsc::Sender<T>, cap: usize) -> Self {
        Self {
            tx: std::sync::Mutex::new(Some(tx)),
            cap,
            dropped: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// 将一条 `ActiveModel` 送入缓冲区。
    ///
    /// 非阻塞，channel 满或已关闭时丢弃并告警（含累计丢弃计数）。
    pub fn send(&self, item: T) {
        // Fast-path：已关闭则直接返回，无需加锁
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let send_result = {
            let guard = self
                .tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match guard.as_ref() {
                Some(tx) => tx.try_send(item),
                None => return,
            }
        };
        if send_result.is_err() {
            let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            warn!(
                target: "monitoring",
                "monitoring data dropped (total: {count}), channel full (cap: {})",
                self.cap
            );
        }
    }

    /// 返回累计丢弃的数据条数。
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 关闭 sender（drop 内部的 Sender，使 channel 关闭）。
    ///
    /// `flush_loop` 的 `rx.recv()` 将收到 `None`，触发最后一次 flush 后退出。
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return; // already closed
        }
        debug!(target: "monitoring", "Closing buffer sender");
        let mut guard = self
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.take(); // drop Sender → channel 关闭 → rx.recv() 返回 None
    }
}

// ── flush loop ──────────────────────────────────────────────────────

/// 后台 flush 循环，定时或按数据量阈值批量写入数据库。
///
/// - `table_name` — 表名，用于日志
/// - `rx` — mpsc 接收端
/// - `flush_interval` — 定时 flush 间隔
/// - `max_batch_size` — 单次批量写入最大行数
///
/// 内部步骤：
/// 1. 等待 tick 或有新数据到达
/// 2. drain 所有立即可用的消息
/// 3. channel 关闭时 flush 剩余数据后退出
async fn flush_loop<E, A>(
    table_name: &'static str,
    mut rx: mpsc::Receiver<A>,
    flush_interval: Duration,
    max_batch_size: usize,
    num_columns: usize,
) where
    E: EntityTrait,
    A: sea_orm::ActiveModelTrait<Entity = E> + Send + 'static,
{
    let mut ticker = interval(flush_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut buf: Vec<A> = Vec::with_capacity(max_batch_size);

    loop {
        // 等待 tick 或有新数据到达
        tokio::select! {
            biased;
            item = rx.recv() => {
                if let Some(model) = item {
                    buf.push(model);
                    // drain 统一在 select! 之后的公共路径完成（recv 与 tick 分支共用）
                } else {
                    // channel 关闭，flush 剩余数据后退出
                    if !buf.is_empty() {
                        do_flush::<E, A>(table_name, &mut buf, num_columns).await;
                    }
                    debug!(target: "monitoring", table = table_name, "Flush loop exiting");
                    return;
                }
            }
            _ = ticker.tick() => {}
        }

        // 公共 drain：recv 分支收到首条后、tick 分支定时触发时都需收集所有立即可用消息
        while buf.len() < max_batch_size {
            match rx.try_recv() {
                Ok(m) => buf.push(m),
                Err(_) => break,
            }
        }

        if !buf.is_empty() {
            trace!(target: "monitoring", table = table_name, buffered = buf.len(), "Flushing buffered items on tick");
            do_flush::<E, A>(table_name, &mut buf, num_columns).await;
        }
    }
}

/// `SQLite` 单条 SQL 语句最多支持的绑定参数数量。
const SQLITE_MAX_VARIABLE_NUMBER: usize = 999;

/// 执行一次批量 INSERT，将缓冲区中的数据写入数据库。
///
/// - `table_name` — 表名，用于日志
/// - `buf` — 待写入的 `ActiveModel` 缓冲区，写入后清空
/// - `num_columns` — 每行 `ActiveModel` 的列数，用于计算 `SQLite` 子批次大小
///
/// 对于 SQLite，根据 `num_columns` 动态计算子批次大小：`999 / num_columns`，
/// 确保单条 INSERT 的绑定参数不超过 `SQLite` 的 999 上限。
/// 对于 PostgreSQL，直接整批写入（无参数数量限制）。
async fn do_flush<E, A>(table_name: &str, buf: &mut Vec<A>, num_columns: usize)
where
    E: EntityTrait,
    A: sea_orm::ActiveModelTrait<Entity = E> + Send + 'static,
{
    let mut batch = std::mem::take(buf);
    let total = batch.len();
    if total == 0 {
        return;
    }

    let Some(db) = ng_db::get_db() else {
        error!(target: "monitoring", table = table_name, count = total, "DB not initialized, dropping batch");
        return;
    };

    let is_sqlite = db.get_database_backend() == sea_orm::DatabaseBackend::Sqlite;

    // SQLite 参数数量限制 (999)，根据列数动态计算每子批次最大行数
    let chunk_size = if is_sqlite {
        let sqlite_batch_limit = SQLITE_MAX_VARIABLE_NUMBER / num_columns.max(1);
        sqlite_batch_limit.min(total)
    } else {
        total
    };

    let mut inserted: usize = 0;
    let mut dropped: usize = 0;

    // 按子批边界零拷贝切分：每次从头部 drain 出 chunk_size 行，避免 chunks().to_vec() 整批 clone。
    // SQLite: 子批次已按 999/num_columns 动态拆分，参数数量不会超限
    // PostgreSQL: chunk_size == total，单次循环整批写入
    while !batch.is_empty() {
        let take = chunk_size.min(batch.len());
        let sub_batch: Vec<A> = batch.drain(..take).collect();
        let count = sub_batch.len();
        // on_conflict_do_nothing：并发冲突时跳过冲突行、保留非冲突行（见 REVIEW M11）。
        // 注意：sea-orm 的 on_conflict_do_nothing() 用 entity 主键生成
        // `ON CONFLICT (id) DO NOTHING`；TimescaleDB 场景（ng-db timescale.rs）把监控表
        // 主键扩展成了 `(id, timestamp)`，`ON CONFLICT (id)` 无约束可匹配 → 整批报错
        // "no unique or exclusion constraint matching the ON CONFLICT specification"。
        // 因此 PostgreSQL 分支直接插入（id 为 identity 自增，并发同刻冲突概率极低；
        // 真冲突时整子批报错丢弃，与 DO NOTHING 语义接近）。SQLite 表主键仍是 id，
        // 保留 on_conflict_do_nothing 防重。
        // on_conflict_do_nothing() 返回 TryInsert、普通 exec 返回 Insert，二者 Result
        // 类型不同，故两分支各自 match 成统一的 Result<(), DbErr> 再合并处理。
        let insert_result: Result<(), sea_orm::DbErr> = if is_sqlite {
            match E::insert_many(sub_batch)
                .on_conflict_do_nothing()
                .exec(db)
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            match E::insert_many(sub_batch).exec(db).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            }
        };
        match insert_result {
            Ok(()) => inserted += count,
            Err(e) => {
                error!(
                    target: "monitoring",
                    table = table_name,
                    count,
                    error = %e,
                    "Batch insert failed, dropping this sub-batch"
                );
                dropped += count;
            }
        }
    }

    if dropped > 0 {
        error!(
            target: "monitoring",
            table = table_name,
            total,
            inserted,
            dropped,
            "Batch insert completed with dropped rows"
        );
    } else {
        debug!(target: "monitoring", table = table_name, count = total, "Batch insert succeeded");
    }
}
