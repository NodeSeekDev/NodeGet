use crate::entity::{dynamic_monitoring, dynamic_monitoring_summary, static_monitoring};
use crate::DB;
use nodeget_lib::config::server::MonitoringBufferConfig;
use sea_orm::EntityTrait;
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use tracing::{debug, error, trace, warn};

// ── 默认值 ──────────────────────────────────────────────────────────

const DEFAULT_FLUSH_INTERVAL_MS: u64 = 500;
const DEFAULT_MAX_BATCH_SIZE: usize = 1000;
const DEFAULT_CHANNEL_CAPACITY: usize = 10000;

// ── 全局单例 ────────────────────────────────────────────────────────

static BUFFERS: OnceLock<MonitoringBuffers> = OnceLock::new();

pub struct MonitoringBuffers {
    pub static_mon: BufferSender<static_monitoring::ActiveModel>,
    pub dynamic_mon: BufferSender<dynamic_monitoring::ActiveModel>,
    pub dynamic_summary: BufferSender<dynamic_monitoring_summary::ActiveModel>,
}

/// 获取全局 buffer 实例
pub fn get() -> &'static MonitoringBuffers {
    BUFFERS
        .get()
        .expect("MonitoringBuffers not initialized — call monitoring_buffer::init() first")
}

/// 初始化全局 buffer 并启动后台 flush task
pub fn init(config: Option<&MonitoringBufferConfig>) {
    let flush_interval_ms = config
        .and_then(|c| c.flush_interval_ms)
        .unwrap_or(DEFAULT_FLUSH_INTERVAL_MS);
    let max_batch_size = config
        .and_then(|c| c.max_batch_size)
        .unwrap_or(DEFAULT_MAX_BATCH_SIZE);

    let flush_interval = Duration::from_millis(flush_interval_ms);

    let (static_tx, static_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
    let (dynamic_tx, dynamic_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
    let (summary_tx, summary_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

    let buffers = MonitoringBuffers {
        static_mon: BufferSender {
            tx: Mutex::new(Some(static_tx)),
        },
        dynamic_mon: BufferSender {
            tx: Mutex::new(Some(dynamic_tx)),
        },
        dynamic_summary: BufferSender {
            tx: Mutex::new(Some(summary_tx)),
        },
    };

    if BUFFERS.set(buffers).is_err() {
        warn!(target: "monitoring", "MonitoringBuffers already initialized, skipping");
        return;
    }

    // 启动三个后台 flush task
    tokio::spawn(flush_loop::<static_monitoring::Entity, static_monitoring::ActiveModel>(
        "static_monitoring",
        static_rx,
        flush_interval,
        max_batch_size,
    ));
    tokio::spawn(flush_loop::<dynamic_monitoring::Entity, dynamic_monitoring::ActiveModel>(
        "dynamic_monitoring",
        dynamic_rx,
        flush_interval,
        max_batch_size,
    ));
    tokio::spawn(flush_loop::<dynamic_monitoring_summary::Entity, dynamic_monitoring_summary::ActiveModel>(
        "dynamic_monitoring_summary",
        summary_rx,
        flush_interval,
        max_batch_size,
    ));

    debug!(
        target: "monitoring",
        flush_interval_ms = flush_interval_ms,
        max_batch_size = max_batch_size,
        "Monitoring write buffers initialized"
    );
}

/// 刷新所有缓冲区并等待完成（用于 graceful shutdown）
///
/// Drop 掉所有 sender 使 channel 关闭，flush_loop 的 `rx.recv()` 返回 `None`，
/// 触发最后一次 flush 后退出。等待足够时间让 flush 完成。
pub async fn flush_and_shutdown() {
    let Some(buffers) = BUFFERS.get() else {
        return;
    };
    // Drop 所有 sender，关闭 channel
    buffers.static_mon.close();
    buffers.dynamic_mon.close();
    buffers.dynamic_summary.close();
    // 等待 flush_loop 完成最后一批写入
    tokio::time::sleep(Duration::from_secs(2)).await;
    debug!(target: "monitoring", "Monitoring buffers shutdown complete");
}

// ── BufferSender ────────────────────────────────────────────────────

pub struct BufferSender<T> {
    tx: Mutex<Option<mpsc::Sender<T>>>,
}

impl<T> BufferSender<T> {
    /// 将一条 ActiveModel 送入缓冲区（非阻塞，满则丢弃并告警）
    pub fn send(&self, item: T) {
        let guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = guard.as_ref() {
            if let Err(_e) = tx.try_send(item) {
                warn!(target: "monitoring", "Buffer channel full or closed, dropping item");
            }
        }
    }

    /// 关闭 sender（drop 内部的 Sender）
    fn close(&self) {
        let mut guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        debug!(target: "monitoring", "Closing buffer sender");
        guard.take(); // drop the sender, closing the channel
    }
}

// ── flush loop ──────────────────────────────────────────────────────

async fn flush_loop<E, A>(
    table_name: &'static str,
    mut rx: mpsc::Receiver<A>,
    flush_interval: Duration,
    max_batch_size: usize,
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
            _ = ticker.tick() => {}
            item = rx.recv() => {
                match item {
                    Some(model) => {
                        buf.push(model);
                        // drain 所有立即可用的消息
                        while buf.len() < max_batch_size {
                            match rx.try_recv() {
                                Ok(m) => buf.push(m),
                                Err(_) => break,
                            }
                        }
                    }
                    None => {
                        // channel 关闭，flush 剩余数据后退出
                        if !buf.is_empty() {
                            do_flush::<E, A>(table_name, &mut buf).await;
                        }
                        debug!(target: "monitoring", table = table_name, "Flush loop exiting");
                        return;
                    }
                }
            }
        }

        // drain 所有立即可用的消息（tick 分支也需要收集）
        while buf.len() < max_batch_size {
            match rx.try_recv() {
                Ok(m) => buf.push(m),
                Err(_) => break,
            }
        }

        if !buf.is_empty() {
            trace!(target: "monitoring", table = table_name, buffered = buf.len(), "Flushing buffered items on tick");
            do_flush::<E, A>(table_name, &mut buf).await;
        }
    }
}

async fn do_flush<E, A>(table_name: &str, buf: &mut Vec<A>)
where
    E: EntityTrait,
    A: sea_orm::ActiveModelTrait<Entity = E> + Send + 'static,
{
    let batch = std::mem::take(buf);
    let count = batch.len();
    if count == 0 {
        return;
    }

    let Some(db) = DB.get() else {
        error!(target: "monitoring", table = table_name, count = count, "DB not initialized, dropping batch");
        return;
    };

    match E::insert_many(batch).exec(db).await {
        Ok(_) => {
            debug!(target: "monitoring", table = table_name, count = count, "Batch insert succeeded");
        }
        Err(e) => {
            error!(target: "monitoring", table = table_name, count = count, error = %e, "Batch insert failed");
        }
    }
}
