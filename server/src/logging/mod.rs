//! 服务器日志系统
//!
//! 基于 tracing + tracing-subscriber 实现，提供四个输出层：
//! 1. 控制台层：自定义格式化器 [`NodeGetFormat`]，支持 ANSI 颜色、target 重映射
//! 2. JSON 文件层（可选）：输出单行 JSON，由 `config.json_log_file` 启用
//! 3. 内存环形缓冲区层：存储最近 N 条日志，供 `nodeget-server.log` RPC 查询
//! 4. 实时流订阅层：通过 [`StreamLogManager`] 广播给 RPC 订阅者
//!
//! 核心设计：
//! - 虚拟 target `db` 自动展开为 `sea_orm`/`sea_orm_migration`/`sqlx` 三个真实 target
//! - 反向映射：`sea_orm*`/`sqlx*` 的日志在输出时统一重映射为 `db`
//! - `StreamLogManager` 使用 `ArcSwap<HashMap>` 替代 `RwLock<HashMap>`，读写均无锁，
//!   彻底消除 `on_event`（读路径）与 `add/remove_subscriber`（写路径）之间的死锁风险

use std::collections::{HashMap, VecDeque};
use std::fmt as stdfmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use ng_config::config::server::LoggingConfig;
use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{
        self, FmtContext, FormattedFields,
        format::{self, FormatEvent, FormatFields},
        time::{ChronoLocal, FormatTime},
    },
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};
use arc_swap::ArcSwap;
use uuid::Uuid;

/// 内存日志环形缓冲区默认容量
const DEFAULT_MEMORY_LOG_CAPACITY: usize = 500;

/// 全局内存日志缓冲区（在 [`init`] 中初始化）
static MEMORY_LOG_BUFFER: OnceLock<Arc<Mutex<VecDeque<serde_json::Value>>>> = OnceLock::new();

/// 内存日志缓冲区最大容量（在 [`init`] 中初始化）
static MEMORY_LOG_CAPACITY: OnceLock<usize> = OnceLock::new();

/// 获取内存日志缓冲区的快照
///
/// 返回当前缓冲区中所有日志条目的克隆列表。
/// 每条日志为 JSON 对象，包含 `timestamp`、`level`、`target`、`message`、`fields`、`spans` 字段。
pub fn get_memory_logs() -> Vec<serde_json::Value> {
    MEMORY_LOG_BUFFER
        .get()
        .map(|buf| {
            let guard = buf
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.iter().cloned().collect()
        })
        .unwrap_or_default()
}

/// 初始化 tracing 日志系统
///
/// 优先级：`RUST_LOG` 环境变量 > `config.log_filter` > 默认 `"info"`。
///
/// 虚拟 target `db` 在过滤器中会自动展开为
/// `sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`。
///
/// 如果配置了 `json_log_file`，会额外输出 JSON 格式日志到该文件，
/// 其过滤器由 `json_log_filter`（或 fallback 到 `log_filter`）控制。
///
/// 内存日志缓冲区默认启用（容量 500），`memory_log_capacity = 0` 表示禁用。
///
/// 注意：如果设置了 `RUST_LOG` 环境变量，它会作为 `json_log_filter` 和
/// `memory_log_filter` 未配置时的 fallback 值，从而同时影响三个输出层。
pub fn init(config: Option<&LoggingConfig>) {
    let default_filter = config
        .and_then(|c| c.log_filter.as_deref())
        .unwrap_or("info");

    // RUST_LOG 环境变量优先级高于配置文件
    let console_raw = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_string());
    let console_expanded = expand_virtual_targets(&console_raw);
    let console_filter = EnvFilter::new(&console_expanded);

    let console_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(true)
        .event_format(NodeGetFormat::new())
        .with_filter(console_filter);

    // ── JSON 文件层（可选）──────────────────────────────────
    let json_layer = config
        .and_then(|c| c.json_log_file.as_deref())
        .and_then(|path| {
            let file = match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("[logging] Failed to open JSON log file {path:?}: {e}");
                    return None;
                }
            };

            let json_filter_raw = config
                .and_then(|c| c.json_log_filter.as_deref())
                .unwrap_or(&console_raw);
            let json_filter_expanded = expand_virtual_targets(json_filter_raw);
            let json_filter = EnvFilter::new(&json_filter_expanded);

            let layer = fmt::layer()
                .json()
                .with_target(true)
                .with_level(true)
                .with_ansi(false)
                .with_writer(Mutex::new(file))
                .event_format(JsonRemapFormat)
                .with_filter(json_filter);

            Some(layer)
        });

    // ── 内存环形缓冲区层 ────────────────────────────────────
    let capacity = config
        .and_then(|c| c.memory_log_capacity)
        .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
    let _ = MEMORY_LOG_CAPACITY.set(capacity);

    // capacity == 0 表示禁用内存日志
    let memory_layer = if capacity > 0 {
        let buffer: Arc<Mutex<VecDeque<serde_json::Value>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        let _ = MEMORY_LOG_BUFFER.set(Arc::clone(&buffer));

        let mem_filter_raw = config
            .and_then(|c| c.memory_log_filter.as_deref())
            .unwrap_or(&console_raw);
        let mem_filter_expanded = expand_virtual_targets(mem_filter_raw);
        let mem_filter = EnvFilter::new(&mem_filter_expanded);

        Some(MemoryLogLayer { buffer }.with_filter(mem_filter))
    } else {
        None
    };

    // ── 实时流订阅层 ─────────────────────────────────────────
    let stream_manager = get_stream_log_manager().clone();
    let stream_layer = StreamLogLayer {
        manager: Arc::clone(&stream_manager),
    }
    .with_filter(StreamLogFilter {
        manager: stream_manager,
    });

    // ── 组装 subscriber ─────────────────────────────────────
    tracing_subscriber::registry()
        .with(console_layer)
        .with(json_layer)
        .with(memory_layer)
        .with(stream_layer)
        .init();
}

// ===========================================================================
//  内存环形缓冲区 Layer
// ===========================================================================

/// 将事件序列化为 JSON 并存入有界环形缓冲区（[`VecDeque`]）的 tracing Layer
///
/// 缓冲区满时淘汰最旧条目。
struct MemoryLogLayer {
    /// 有界环形缓冲区，存储 JSON 格式日志事件
    buffer: Arc<Mutex<VecDeque<serde_json::Value>>>,
}

impl<S> Layer<S> for MemoryLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // 内存日志收集：采集事件字段 → 组装 span 上下文 → 构建日志条目 → 写入有界缓冲区
        // 1. JsonFieldVisitor 收集事件的结构化字段与 message
        // 2. 遍历 event_scope 中所有 span，用直接 Map 构造替代 json! 宏避免中间 Value 分配
        // 3. build_log_entry 组装完整日志条目
        // 4. 写入 VecDeque 缓冲区，超限时从队首弹出；使用 unwrap_or_else(into_inner) 从 Mutex 中毒恢复
        let meta = event.metadata();

        // 收集结构化字段
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.message.take().unwrap_or_default();

        // 收集 span 上下文——直接 Map 构造替代 serde_json::json!，消除每 span 的中间 Value 分配
        let spans: Vec<serde_json::Value> = ctx
            .event_scope(event)
            .into_iter()
            .flatten()
            .map(|span| {
                let mut map = serde_json::Map::with_capacity(2);
                map.insert(
                    "name".to_owned(),
                    serde_json::Value::String(span.name().to_owned()),
                );
                let ext = span.extensions();
                if let Some(fields) = ext
                    .get::<FormattedFields<format::DefaultFields>>()
                    .filter(|f| !f.is_empty())
                {
                    map.insert(
                        "fields".to_owned(),
                        serde_json::Value::String(strip_ansi(&fields.to_string())),
                    );
                }
                drop(ext);
                serde_json::Value::Object(map)
            })
            .collect();

        // 直接 Map 构造替代 serde_json::json!，消除 6 次中间 Value 分配
        let entry = build_log_entry(meta, message, visitor.fields, spans);

        // 使用 unwrap_or_else(into_inner) 从 Mutex 中毒中恢复，
        // 而非静默丢弃日志条目
        let mut guard = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cap = MEMORY_LOG_CAPACITY
            .get()
            .copied()
            .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
        // cap 在 init 中保证 > 0，此处防御性检查
        if cap > 0 {
            while guard.len() >= cap {
                guard.pop_front();
            }
            guard.push_back(entry);
        }
    }
}

// ---------------------------------------------------------------------------
//  字段访问器——将事件字段收集为 JSON Map
// ---------------------------------------------------------------------------

/// tracing `Visit` 实现，将事件字段收集为 JSON Map
///
/// - message：特殊处理，提取到顶层 `message` 字段
/// - fields：其余字段存入 `fields` JSON 对象
#[derive(Default)]
struct JsonFieldVisitor {
    /// 事件的 `message` 字段值
    message: Option<String>,
    /// 除 message 外的所有字段
    fields: serde_json::Map<String, serde_json::Value>,
}

impl Visit for JsonFieldVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn stdfmt::Debug) {
        let val = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(val);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(val));
        }
    }
}

// ===========================================================================
//  虚拟 target 展开
// ===========================================================================

/// 展开 `EnvFilter` 字符串中的虚拟 target 别名
///
/// 当前支持的别名：
/// - `db=<level>` → `db=<level>,sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`
///
/// 保留字面 `db` 指令，使得使用 `target: "db"` 的自有代码也能被过滤器匹配。
/// 非别名指令原样透传。
fn expand_virtual_targets(filter: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    for directive in filter.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }

        if let Some(level) = directive.strip_prefix("db=") {
            // 保留字面 "db=<level>"，使 target: "db" 的事件也能匹配
            parts.push(format!("db={level}"));
            parts.push(format!("sea_orm={level}"));
            parts.push(format!("sea_orm_migration={level}"));
            parts.push(format!("sqlx={level}"));
        } else if directive == "db" {
            parts.push("db".to_string());
            parts.push("sea_orm".to_string());
            parts.push("sea_orm_migration".to_string());
            parts.push("sqlx".to_string());
        } else {
            parts.push(directive.to_string());
        }
    }

    parts.join(",")
}

// ===========================================================================
//  target 重映射
// ===========================================================================

/// 将已知的数据库相关 log target 映射为 `"db"`
fn remap_target(target: &str) -> &str {
    if target.starts_with("sea_orm") || target.starts_with("sqlx") {
        "db"
    } else {
        target
    }
}

// ===========================================================================
//  自定义控制台格式化器
// ===========================================================================

/// 自定义事件格式化器：支持 target 重映射和 ANSI 颜色
///
/// 输出格式：`<时间戳> <级别> <target>: <字段> [span<fields>]`
struct NodeGetFormat {
    /// 时间戳格式化器
    timer: ChronoLocal,
}

impl NodeGetFormat {
    fn new() -> Self {
        Self {
            timer: ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f%:z".to_string()),
        }
    }
}

impl<S, N> FormatEvent<S, N> for NodeGetFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: format::Writer<'_>,
        event: &Event<'_>,
    ) -> stdfmt::Result {
        // 控制台单行格式化：时间戳 → 彩色级别 → target 重映射 → 字段 → span 上下文
        // 1. timer 输出时间戳
        // 2. 根据 ANSI 支持决定是否着色级别（5 字符右对齐）
        // 3. remap_target 映射模块路径为友好名称，灰色显示
        // 4. format_fields 渲染事件字段
        // 5. 遍历 event_scope 拼接 [span{fields} < span{fields}] 格式的 span 上下文
        // 时间戳
        self.timer.format_time(&mut writer)?;

        // 级别（带颜色）
        let level = *event.metadata().level();
        if writer.has_ansi_escapes() {
            let (open, close) = level_ansi(level);
            write!(writer, " {open}{level:>5}{close} ")?;
        } else {
            write!(writer, " {level:>5} ")?;
        }

        // target（重映射后，灰色显示）
        let raw_target = event.metadata().target();
        let target = remap_target(raw_target);
        if writer.has_ansi_escapes() {
            write!(writer, "\x1b[2m{target}\x1b[0m: ")?;
        } else {
            write!(writer, "{target}: ")?;
        }

        // 字段
        ctx.format_fields(writer.by_ref(), event)?;

        // span 上下文（单行显示）
        if let Some(scope) = ctx.event_scope() {
            let mut first = true;
            for span in scope {
                let ext = span.extensions();
                let has_fields = ext
                    .get::<FormattedFields<N>>()
                    .is_some_and(|f| !f.is_empty());
                if first {
                    write!(writer, " [")?;
                    first = false;
                } else {
                    write!(writer, " < ")?;
                }
                if has_fields {
                    let fields = ext.get::<FormattedFields<N>>().unwrap();
                    write!(writer, "{}{{{fields}}}", span.name())?;
                } else {
                    write!(writer, "{}", span.name())?;
                }
                drop(ext);
            }
            if !first {
                write!(writer, "]")?;
            }
        }

        writeln!(writer)
    }
}

// ===========================================================================
//  JSON 文件格式化器（带 target 重映射）
// ===========================================================================

/// 自定义 JSON 事件格式化器，应用 [`remap_target`] 后序列化
///
/// 确保 JSON 文件输出与控制台/内存层的 target 命名一致。
struct JsonRemapFormat;

impl<S, N> FormatEvent<S, N> for JsonRemapFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: format::Writer<'_>,
        event: &Event<'_>,
    ) -> stdfmt::Result {
        // JSON 文件格式化：采集字段 → 构建 span 上下文 → 组装日志条目 → 输出单行 JSON
        // 1. JsonFieldVisitor 收集事件字段与 message
        // 2. 遍历 span 链用直接 Map 构造上下文对象，strip_ansi 清除 ANSI 转义
        // 3. build_log_entry 统一组装（含 remap_target），保证与控制台层 target 命名一致
        // 4. 写入单行 JSON + 换行
        let meta = event.metadata();
        // 收集字段
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.take().unwrap_or_default();

        // 收集 span 上下文——直接 Map 构造替代 serde_json::json!
        let spans: Vec<serde_json::Value> = ctx
            .event_scope()
            .into_iter()
            .flatten()
            .map(|span| {
                let mut map = serde_json::Map::with_capacity(2);
                map.insert(
                    "name".to_owned(),
                    serde_json::Value::String(span.name().to_owned()),
                );
                let ext = span.extensions();
                if let Some(fields) = ext.get::<FormattedFields<N>>().filter(|f| !f.is_empty()) {
                    map.insert(
                        "fields".to_owned(),
                        serde_json::Value::String(strip_ansi(&fields.to_string())),
                    );
                }
                drop(ext);
                serde_json::Value::Object(map)
            })
            .collect();

        let entry = build_log_entry(meta, message, visitor.fields, spans);

        // 输出单行 JSON（无尾逗号）
        write!(writer, "{entry}")?;
        writeln!(writer)
    }
}

// ===========================================================================
//  辅助函数
// ===========================================================================

/// 返回给定 tracing 级别对应的 ANSI 转义码对（开启码，重置码）
const fn level_ansi(level: tracing::Level) -> (&'static str, &'static str) {
    const RESET: &str = "\x1b[0m";
    match level {
        tracing::Level::ERROR => ("\x1b[31m", RESET), // 红色
        tracing::Level::WARN => ("\x1b[33m", RESET),  // 黄色
        tracing::Level::INFO => ("\x1b[32m", RESET),  // 绿色
        tracing::Level::DEBUG => ("\x1b[34m", RESET), // 蓝色
        tracing::Level::TRACE => ("\x1b[35m", RESET), // 紫色
    }
}

/// 构建 JSON 日志条目，供 `MemoryLogLayer::on_event` 和 `JsonRemapFormat::format_event` 共享。
///
/// 直接 Map 构造替代 `serde_json::json!`，消除每条日志 6 次中间 `Value` 分配。
fn build_log_entry(
    meta: &tracing::Metadata<'_>,
    message: String,
    fields: serde_json::Map<String, serde_json::Value>,
    spans: Vec<serde_json::Value>,
) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(6);
    map.insert(
        "timestamp".to_owned(),
        serde_json::Value::String(
            chrono::Local::now()
                .format("%Y-%m-%dT%H:%M:%S%.3f%:z")
                .to_string(),
        ),
    );
    map.insert(
        "level".to_owned(),
        serde_json::Value::String(meta.level().as_str().to_owned()),
    );
    map.insert(
        "target".to_owned(),
        serde_json::Value::String(remap_target(meta.target()).to_owned()),
    );
    map.insert("message".to_owned(), serde_json::Value::String(message));
    map.insert("fields".to_owned(), serde_json::Value::Object(fields));
    map.insert("spans".to_owned(), serde_json::Value::Array(spans));
    serde_json::Value::Object(map)
}

/// 剥离字符串中的 ANSI 转义序列
///
/// 需要此函数是因为控制台层的 `FormattedFields<DefaultFields>`
/// 包含 ANSI 颜色/样式码（斜体、暗淡、重置等），
/// 这些码不能泄漏到 JSON 文件输出或内存日志缓冲区中。
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // 消费 '[' 及后续参数字节，直到终结字母
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ===========================================================================
//  实时流日志——基于 RPC 的日志订阅
// ===========================================================================

/// 全局单例，管理所有活跃的流日志订阅者
static STREAM_LOG_MANAGER: OnceLock<Arc<StreamLogManager>> = OnceLock::new();

/// 返回全局 [`StreamLogManager`] 单例（首次调用时创建）
pub fn get_stream_log_manager() -> &'static Arc<StreamLogManager> {
    STREAM_LOG_MANAGER.get_or_init(|| Arc::new(StreamLogManager::new()))
}

/// 管理所有活跃的流日志订阅者
///
/// 使用 `ArcSwap<HashMap>` 替代 `RwLock<HashMap>`：
/// - 读路径（`on_event`）通过 `load()` 获取 `Arc` 引用，零开销无锁，
///   不会与写路径产生任何竞争
/// - 写路径（`add/remove_subscriber`）通过 `rcu()` 原子替换整个 `HashMap`，
///   无需持有锁，因此可以安全地调用 tracing 而不会死锁
/// - `subscriber_count` 原子计数器提供快速路径：订阅者为零时跳过 `load()`
pub struct StreamLogManager {
    /// 订阅者映射表（UUID → 订阅者），原子替换
    subscribers: ArcSwap<HashMap<Uuid, StreamLogSubscriber>>,
    /// 订阅者数量（快速路径优化：为零时避免 load 开销）
    subscriber_count: AtomicUsize,
}

impl StreamLogManager {
    fn new() -> Self {
        Self {
            subscribers: ArcSwap::new(Arc::new(HashMap::new())),
            subscriber_count: AtomicUsize::new(0),
        }
    }

    /// 注册新订阅者
    ///
    /// - id：订阅者唯一标识
    /// - tx：日志条目发送通道
    /// - `filter_str`：`EnvFilter` 格式的过滤器字符串
    ///
    /// 使用 `rcu()` 原子替换 HashMap，无需持锁，可安全调用 tracing。
    pub fn add_subscriber(
        &self,
        id: Uuid,
        tx: tokio::sync::mpsc::Sender<String>,
        filter_str: &str,
    ) {
        let expanded = expand_virtual_targets(filter_str);
        let filter = StreamFilter::parse(&expanded);
        let subscriber = StreamLogSubscriber { tx, filter };
        self.subscribers.rcu(|current| {
            let mut new_map = (**current).clone();
            new_map.insert(id, subscriber.clone());
            new_map
        });
        self.subscriber_count
            .store(self.subscribers.load().len(), Ordering::Release);
    }

    /// 按 id 移除订阅者
    ///
    /// 使用 `rcu()` 原子替换 HashMap，无需持锁，可安全调用 tracing。
    pub fn remove_subscriber(&self, id: &Uuid) {
        self.subscribers.rcu(|current| {
            let mut new_map = (**current).clone();
            new_map.remove(id);
            new_map
        });
        self.subscriber_count
            .store(self.subscribers.load().len(), Ordering::Release);
    }

    /// 是否存在至少一个活跃订阅者
    #[inline]
    fn has_subscribers(&self) -> bool {
        self.subscriber_count.load(Ordering::Acquire) > 0
    }
}

/// 单个流日志订阅者，持有独立的过滤器和发送通道
#[derive(Clone)]
struct StreamLogSubscriber {
    /// 日志条目发送通道（预序列化 JSON 字符串）
    tx: tokio::sync::mpsc::Sender<String>,
    /// 订阅者专属过滤器
    filter: StreamFilter,
}

// ---------------------------------------------------------------------------
//  StreamFilter——轻量级 target+level 匹配器
// ---------------------------------------------------------------------------

/// 轻量级过滤器，按 target 前缀和级别匹配事件
///
/// 支持 `RUST_LOG` / `EnvFilter` 相同的 `target=level` 指令格式，
/// 但仅处理 target+level 匹配（无 span 过滤）。
#[derive(Clone)]
struct StreamFilter {
    /// 无 target 指令匹配时的默认级别
    default_level: tracing::level_filters::LevelFilter,
    /// 按 target 前缀的级别覆盖，按长度降序排列以实现最长前缀匹配
    targets: Vec<(String, tracing::level_filters::LevelFilter)>,
}

impl StreamFilter {
    /// 将 `EnvFilter` 兼容的过滤器字符串解析为 [`StreamFilter`]
    ///
    /// 接受如 `"info"`、`"server=debug,rpc=trace"`、`"warn,server=info"` 等指令。
    /// 无法识别的级别字符串被静默忽略。
    fn parse(filter_str: &str) -> Self {
        let mut default_level = tracing::level_filters::LevelFilter::OFF;
        let mut targets = Vec::new();

        for directive in filter_str.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }

            if let Some((target, level_str)) = directive.split_once('=') {
                let target = target.trim();
                let level_str = level_str.trim();
                if let Some(level) = parse_level_filter(level_str) {
                    targets.push((target.to_string(), level));
                }
            } else if let Some(level) = parse_level_filter(directive) {
                // 裸级别如 "info" 设置默认值
                default_level = level;
            }
        }

        // 按 target 长度降序排列，实现最长前缀匹配
        targets.sort_by_key(|b| std::cmp::Reverse(b.0.len()));

        Self {
            default_level,
            targets,
        }
    }

    /// 判断给定元数据是否通过此过滤器
    fn is_enabled(&self, meta: &Metadata<'_>) -> bool {
        let target = meta.target();
        let level = meta.level();

        // 最长前缀匹配
        for (prefix, filter_level) in &self.targets {
            if target.starts_with(prefix.as_str()) {
                return level <= filter_level;
            }
        }

        // 回退到默认级别
        level <= &self.default_level
    }
}

/// 将级别字符串（不区分大小写）解析为 [`LevelFilter`]
fn parse_level_filter(s: &str) -> Option<tracing::level_filters::LevelFilter> {
    match s.to_lowercase().as_str() {
        "off" => Some(tracing::level_filters::LevelFilter::OFF),
        "error" => Some(tracing::level_filters::LevelFilter::ERROR),
        "warn" => Some(tracing::level_filters::LevelFilter::WARN),
        "info" => Some(tracing::level_filters::LevelFilter::INFO),
        "debug" => Some(tracing::level_filters::LevelFilter::DEBUG),
        "trace" => Some(tracing::level_filters::LevelFilter::TRACE),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
//  StreamLogFilter——per-layer 过滤器（Filter<S> trait）
// ---------------------------------------------------------------------------

/// [`StreamLogLayer`] 的 per-layer 过滤器
///
/// **必须**作为 per-layer filter 使用（通过 `.with_filter()`），
/// 而非全局过滤器。因为 `Layered` subscriber 的 AND 逻辑下，
/// 若某 Layer 的 `enabled()` 返回 false 会阻塞**所有其他 Layer** 接收该事件。
///
/// 此过滤器仅检查是否存在订阅者（`subscriber_count > 0`），
/// per-subscriber 过滤在 `StreamLogLayer::on_event` 内完成。
struct StreamLogFilter {
    /// 流日志订阅管理器
    manager: Arc<StreamLogManager>,
}

impl<S> tracing_subscriber::layer::Filter<S> for StreamLogFilter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(
        &self,
        _meta: &Metadata<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        // 快速路径：单次原子加载
        self.manager.has_subscribers()
    }
}

// ---------------------------------------------------------------------------
//  StreamLogLayer——将事件广播给订阅者
// ---------------------------------------------------------------------------

/// 将事件广播给所有活跃流日志订阅者的 tracing Layer
///
/// 使用与 [`MemoryLogLayer`] 相同的 JSON 格式序列化事件，
/// 通过 `try_send`（非阻塞）发送，避免慢订阅者的背压。
struct StreamLogLayer {
    /// 流日志订阅管理器
    manager: Arc<StreamLogManager>,
}

impl<S> Layer<S> for StreamLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // WebSocket 流日志推送：快速路径过滤 → 无锁快照筛选订阅者 → 序列化 → 广播预序列化 JSON
        // 1. has_subscribers() 快速路径：无订阅者直接返回，避免不必要的字段采集与序列化
        // 2. ArcSwap::load() 获取 Arc<HashMap> 快照，无锁遍历，filter.is_enabled(meta) 收集感兴趣的通道
        // 3. JsonFieldVisitor 采集字段，遍历 span 链构建上下文，remap_target 映射模块路径
        // 4. serde_json::json! 组装完整条目，to_string 序列化一次
        // 5. try_send 将预序列化 JSON 字符串克隆广播给所有订阅者，避免 per-subscriber 深拷贝
        // 快速路径：无订阅者
        if !self.manager.has_subscribers() {
            return;
        }

        let meta = event.metadata();

        // 获取订阅者映射表快照（Arc 引用，无锁）
        let subscribers = self.manager.subscribers.load();

        if subscribers.is_empty() {
            return;
        }

        // 预过滤：收集对此事件感兴趣的订阅者通道
        let interested_tx: Vec<tokio::sync::mpsc::Sender<String>> = subscribers
            .values()
            .filter(|sub| sub.filter.is_enabled(meta))
            .map(|sub| sub.tx.clone())
            .collect();

        drop(subscribers);

        if interested_tx.is_empty() {
            return;
        }

        // 序列化事件（与 MemoryLogLayer 相同格式）
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.take().unwrap_or_default();

        let spans: Vec<serde_json::Value> = ctx
            .event_scope(event)
            .into_iter()
            .flatten()
            .map(|span| {
                let mut obj = serde_json::json!({ "name": span.name() });
                let ext = span.extensions();
                if let Some(fields) = ext
                    .get::<FormattedFields<format::DefaultFields>>()
                    .filter(|f| !f.is_empty())
                {
                    obj["fields"] = serde_json::Value::String(strip_ansi(&fields.to_string()));
                }
                drop(ext);
                obj
            })
            .collect();

        let target = remap_target(meta.target());

        let entry = serde_json::json!({
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            "level": meta.level().as_str(),
            "target": target,
            "message": message,
            "fields": visitor.fields,
            "spans": spans,
        });

        // 序列化一次，广播预序列化 JSON 字符串给所有订阅者（避免 per-subscriber 深拷贝）
        let Ok(json_str) = serde_json::to_string(&entry) else {
            return;
        };

        for tx in interested_tx {
            let _ = tx.try_send(json_str.clone());
        }
    }
}
