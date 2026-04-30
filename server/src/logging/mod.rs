use std::collections::{HashMap, VecDeque};
use std::fmt as stdfmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use nodeget_lib::config::server::LoggingConfig;
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
use uuid::Uuid;

/// Default capacity for the in-memory log ring buffer.
const DEFAULT_MEMORY_LOG_CAPACITY: usize = 500;

/// Global handle to the in-memory log buffer (initialised once in [`init`]).
static MEMORY_LOG_BUFFER: OnceLock<Arc<Mutex<VecDeque<serde_json::Value>>>> = OnceLock::new();

/// Maximum capacity for the memory log buffer (initialised once in [`init`]).
static MEMORY_LOG_CAPACITY: OnceLock<usize> = OnceLock::new();

/// Returns a snapshot of all log entries currently held in the memory buffer.
///
/// Each entry is a JSON object with fields: `timestamp`, `level`, `target`,
/// `message`, `fields`, `spans`.
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

    // RUST_LOG env var overrides config
    let console_raw = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_string());
    let console_expanded = expand_virtual_targets(&console_raw);
    let console_filter = EnvFilter::new(&console_expanded);

    let console_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(true)
        .event_format(NodeGetFormat::new())
        .with_filter(console_filter);

    // ── JSON file layer (optional) ──────────────────────────────────
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

    // ── Memory ring-buffer layer ────────────────────────────────────
    let capacity = config
        .and_then(|c| c.memory_log_capacity)
        .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
    let _ = MEMORY_LOG_CAPACITY.set(capacity);

    // capacity == 0 means the memory log feature is disabled
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

    // ── Stream log layer (real-time subscription) ─────────────────
    let stream_manager = get_stream_log_manager().clone();
    let stream_layer = StreamLogLayer {
        manager: Arc::clone(&stream_manager),
    }
    .with_filter(StreamLogFilter {
        manager: stream_manager,
    });

    // ── Assemble subscriber ─────────────────────────────────────────
    tracing_subscriber::registry()
        .with(console_layer)
        .with(json_layer)
        .with(memory_layer)
        .with(stream_layer)
        .init();
}

// ===========================================================================
//  In-memory ring-buffer layer
// ===========================================================================

/// A [`Layer`] that serialises each event to JSON and
/// stores it in a bounded ring buffer ([`VecDeque`]).
///
/// When the buffer reaches capacity the oldest entry is evicted.
struct MemoryLogLayer {
    buffer: Arc<Mutex<VecDeque<serde_json::Value>>>,
}

impl<S> Layer<S> for MemoryLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let meta = event.metadata();

        // Collect structured fields
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.message.take().unwrap_or_default();

        // Collect span context — strip ANSI because `FormattedFields<DefaultFields>`
        // is stored by the console layer which has `with_ansi(true)`.
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

        // Use unwrap_or_else(into_inner) to recover from Mutex poisoning
        // instead of silently dropping the log entry.
        let mut guard = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cap = MEMORY_LOG_CAPACITY
            .get()
            .copied()
            .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
        // cap is guaranteed > 0 (checked in init), but defend against
        // unexpected edge cases by requiring cap > 0.
        if cap > 0 {
            while guard.len() >= cap {
                guard.pop_front();
            }
            guard.push_back(entry);
        }
    }
}

// ---------------------------------------------------------------------------
//  Field visitor – collects event fields into a JSON map
// ---------------------------------------------------------------------------

#[derive(Default)]
struct JsonFieldVisitor {
    message: Option<String>,
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
//  Virtual target expansion
// ===========================================================================

/// Expands virtual target aliases in an `EnvFilter`-compatible string.
///
/// Currently supported aliases:
/// - `db=<level>` → `db=<level>,sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`
///
/// The literal `db` directive is preserved so that our own code using
/// `target: "db"` is also matched by the filter.
///
/// Directives that are not aliases are passed through unchanged.
fn expand_virtual_targets(filter: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    for directive in filter.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }

        if let Some(level) = directive.strip_prefix("db=") {
            // Keep literal "db=<level>" so our own `target: "db"` events match
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
//  Target remapping
// ===========================================================================

/// Maps known database-related log targets to `"db"`.
fn remap_target(target: &str) -> &str {
    if target.starts_with("sea_orm") || target.starts_with("sqlx") {
        "db"
    } else {
        target
    }
}

// ===========================================================================
//  Custom console formatter
// ===========================================================================

/// Custom event format with target remapping and ANSI colours.
struct NodeGetFormat {
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
        // Timestamp
        self.timer.format_time(&mut writer)?;

        // Level
        let level = *event.metadata().level();
        if writer.has_ansi_escapes() {
            let (open, close) = level_ansi(level);
            write!(writer, " {open}{level:>5}{close} ")?;
        } else {
            write!(writer, " {level:>5} ")?;
        }

        // Target (remapped)
        let raw_target = event.metadata().target();
        let target = remap_target(raw_target);
        if writer.has_ansi_escapes() {
            write!(writer, "\x1b[2m{target}\x1b[0m: ")?;
        } else {
            write!(writer, "{target}: ")?;
        }

        // Fields
        ctx.format_fields(writer.by_ref(), event)?;

        // Span context (single-line)
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
//  JSON file format with target remapping
// ===========================================================================

/// A custom JSON event format that applies [`remap_target`] before serialising,
/// ensuring the JSON file output is consistent with console and memory layers.
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
        let meta = event.metadata();
        let target = remap_target(meta.target());

        // Collect fields
        let mut visitor = JsonFieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.take().unwrap_or_default();

        // Collect span context — defensive strip_ansi in case the field
        // formatter type `N` shares storage with an ANSI-enabled layer.
        let spans: Vec<serde_json::Value> = ctx
            .event_scope()
            .into_iter()
            .flatten()
            .map(|span| {
                let mut obj = serde_json::json!({ "name": span.name() });
                let ext = span.extensions();
                if let Some(fields) = ext.get::<FormattedFields<N>>().filter(|f| !f.is_empty()) {
                    obj["fields"] = serde_json::Value::String(strip_ansi(&fields.to_string()));
                }
                drop(ext);
                obj
            })
            .collect();

        let entry = serde_json::json!({
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            "level": meta.level().as_str(),
            "target": target,
            "message": message,
            "fields": visitor.fields,
            "spans": spans,
        });

        // Write a single line of JSON (no trailing comma)
        write!(writer, "{entry}")?;
        writeln!(writer)
    }
}

// ===========================================================================
//  Helpers
// ===========================================================================

/// ANSI escape pair `(open, reset)` for the given tracing level.
const fn level_ansi(level: tracing::Level) -> (&'static str, &'static str) {
    const RESET: &str = "\x1b[0m";
    match level {
        tracing::Level::ERROR => ("\x1b[31m", RESET),
        tracing::Level::WARN => ("\x1b[33m", RESET),
        tracing::Level::INFO => ("\x1b[32m", RESET),
        tracing::Level::DEBUG => ("\x1b[34m", RESET),
        tracing::Level::TRACE => ("\x1b[35m", RESET),
    }
}

/// Strips ANSI escape sequences (`ESC[...X`) from a string.
///
/// This is needed because `FormattedFields<DefaultFields>` stored by the
/// console layer includes ANSI colour/style codes (italic, dim, reset, etc.)
/// which must not leak into JSON file output or the memory log buffer.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Consume the '[' and all parameter bytes until a final letter
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
//  Stream log – real-time log subscription via RPC
// ===========================================================================

/// Global singleton managing all active stream log subscribers.
static STREAM_LOG_MANAGER: OnceLock<Arc<StreamLogManager>> = OnceLock::new();

/// Returns the global [`StreamLogManager`] singleton (created on first call).
pub fn get_stream_log_manager() -> &'static Arc<StreamLogManager> {
    STREAM_LOG_MANAGER.get_or_init(|| Arc::new(StreamLogManager::new()))
}

/// Manages all active stream log subscribers.
///
/// Uses `std::sync::RwLock` because it is accessed from the synchronous
/// `on_event` callback in the tracing layer. The `subscriber_count` atomic
/// provides a fast path to skip lock acquisition when there are no subscribers.
pub struct StreamLogManager {
    subscribers: RwLock<HashMap<Uuid, StreamLogSubscriber>>,
    /// Fast-path optimisation: avoids acquiring the read lock when zero.
    subscriber_count: AtomicUsize,
}

impl StreamLogManager {
    fn new() -> Self {
        Self {
            subscribers: RwLock::new(HashMap::new()),
            subscriber_count: AtomicUsize::new(0),
        }
    }

    /// Register a new subscriber.
    ///
    /// **WARNING**: Do NOT emit any tracing events while calling this method –
    /// it holds the write lock, and `on_event` acquires the read lock, which
    /// would deadlock on non-reentrant `std::sync::RwLock`.
    pub fn add_subscriber(
        &self,
        id: Uuid,
        tx: tokio::sync::mpsc::Sender<serde_json::Value>,
        filter_str: &str,
    ) {
        let expanded = expand_virtual_targets(filter_str);
        let filter = StreamFilter::parse(&expanded);
        let subscriber = StreamLogSubscriber { tx, filter };
        let mut guard = self
            .subscribers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.insert(id, subscriber);
        self.subscriber_count.store(guard.len(), Ordering::Release);
    }

    /// Remove a subscriber by id.
    ///
    /// **WARNING**: Same deadlock caveat as [`add_subscriber`].
    pub fn remove_subscriber(&self, id: &Uuid) {
        let mut guard = self
            .subscribers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.remove(id);
        self.subscriber_count.store(guard.len(), Ordering::Release);
    }

    /// Returns `true` if there is at least one active subscriber.
    #[inline]
    fn has_subscribers(&self) -> bool {
        self.subscriber_count.load(Ordering::Acquire) > 0
    }
}

/// A single stream log subscriber with its own filter and channel.
struct StreamLogSubscriber {
    tx: tokio::sync::mpsc::Sender<serde_json::Value>,
    filter: StreamFilter,
}

// ---------------------------------------------------------------------------
//  StreamFilter – lightweight target+level matcher
// ---------------------------------------------------------------------------

/// A lightweight filter that matches events by target prefix and level.
///
/// Supports the same `target=level` directive format as `RUST_LOG` / `EnvFilter`,
/// but only handles target+level matching (no span-based filtering).
struct StreamFilter {
    /// Default level when no target directive matches.
    default_level: tracing::level_filters::LevelFilter,
    /// Per-target level overrides, sorted by decreasing length for longest-prefix match.
    targets: Vec<(String, tracing::level_filters::LevelFilter)>,
}

impl StreamFilter {
    /// Parses an `EnvFilter`-compatible filter string into a [`StreamFilter`].
    ///
    /// Accepts directives like `"info"`, `"server=debug,rpc=trace"`,
    /// `"warn,server=info"`, etc. Unknown level strings are silently ignored.
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
                // Bare level like "info" sets the default
                default_level = level;
            }
        }

        // Sort by target length descending for longest-prefix match
        targets.sort_by_key(|b| std::cmp::Reverse(b.0.len()));

        Self {
            default_level,
            targets,
        }
    }

    /// Returns `true` if the given metadata passes this filter.
    fn is_enabled(&self, meta: &Metadata<'_>) -> bool {
        let target = meta.target();
        let level = meta.level();

        // Longest prefix match
        for (prefix, filter_level) in &self.targets {
            if target.starts_with(prefix.as_str()) {
                return level <= filter_level;
            }
        }

        // Fall back to default
        level <= &self.default_level
    }
}

/// Parses a level string (case-insensitive) into a [`LevelFilter`].
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
//  StreamLogFilter – per-layer filter (Filter<S> trait)
// ---------------------------------------------------------------------------

/// Per-layer filter for [`StreamLogLayer`].
///
/// This **must** be used as a per-layer filter (via `.with_filter()`), not as a
/// global filter. Without per-layer filtering, a `Layer` whose `enabled()`
/// returns `false` would block **all other layers** from receiving the event
/// due to the `Layered` subscriber's AND logic.
///
/// The filter checks only whether any subscribers exist (`subscriber_count > 0`).
/// Per-subscriber filtering is done inside `StreamLogLayer::on_event`.
struct StreamLogFilter {
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
        // Fast path: single atomic load
        self.manager.has_subscribers()
    }
}

// ---------------------------------------------------------------------------
//  StreamLogLayer – broadcasts events to subscribers
// ---------------------------------------------------------------------------

/// A [`Layer`] that broadcasts events to all active stream log subscribers.
///
/// Serialises events in the same JSON format as [`MemoryLogLayer`] and uses
/// `try_send` (non-blocking) to avoid back-pressure from slow subscribers.
struct StreamLogLayer {
    manager: Arc<StreamLogManager>,
}

impl<S> Layer<S> for StreamLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Fast path: no subscribers
        if !self.manager.has_subscribers() {
            return;
        }

        let meta = event.metadata();

        // Acquire read lock and find which subscribers are interested
        let guard = self
            .manager
            .subscribers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if guard.is_empty() {
            return;
        }

        // Pre-filter: collect subscribers interested in this event
        let interested_tx: Vec<tokio::sync::mpsc::Sender<serde_json::Value>> = guard
            .values()
            .filter(|sub| sub.filter.is_enabled(meta))
            .map(|sub| sub.tx.clone())
            .collect();

        drop(guard);

        if interested_tx.is_empty() {
            return;
        }

        // Serialise the event (same format as MemoryLogLayer)
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

        // Broadcast to all interested subscribers (non-blocking)
        for tx in interested_tx {
            let _ = tx.try_send(entry.clone());
        }
    }
}
