use std::collections::VecDeque;
use std::fmt as stdfmt;
use std::sync::{Arc, Mutex, OnceLock};

use nodeget_lib::config::server::LoggingConfig;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{
        self,
        format::{self, FormatEvent, FormatFields},
        time::{ChronoLocal, FormatTime},
        FmtContext, FormattedFields,
    },
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

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
            let guard = buf.lock().expect("memory log buffer poisoned");
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
/// 始终启用内存日志缓冲区，可通过 `memory_log_capacity` 和
/// `memory_log_filter` 配置容量与过滤级别。
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
        .event_format(NodeGetFormat::new());

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
                .with_writer(std::sync::Mutex::new(file))
                .with_filter(json_filter);

            Some(layer)
        });

    // ── Memory ring-buffer layer ────────────────────────────────────
    let capacity = config
        .and_then(|c| c.memory_log_capacity)
        .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
    let _ = MEMORY_LOG_CAPACITY.set(capacity);

    let buffer: Arc<Mutex<VecDeque<serde_json::Value>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
    let _ = MEMORY_LOG_BUFFER.set(Arc::clone(&buffer));

    let mem_filter_raw = config
        .and_then(|c| c.memory_log_filter.as_deref())
        .unwrap_or(&console_raw);
    let mem_filter_expanded = expand_virtual_targets(mem_filter_raw);
    let mem_filter = EnvFilter::new(&mem_filter_expanded);

    let memory_layer = MemoryLogLayer { buffer }.with_filter(mem_filter);

    // ── Assemble subscriber ─────────────────────────────────────────
    tracing_subscriber::registry()
        .with(console_filter)
        .with(console_layer)
        .with(json_layer)
        .with(memory_layer)
        .init();
}

// ===========================================================================
//  In-memory ring-buffer layer
// ===========================================================================

/// A [`tracing_subscriber::Layer`] that serialises each event to JSON and
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

        // Collect span context
        let spans: Vec<serde_json::Value> = ctx
            .event_scope(event)
            .into_iter()
            .flatten()
            .map(|span| {
                let mut obj = serde_json::json!({ "name": span.name() });
                let ext = span.extensions();
                if let Some(fields) = ext
                    .get::<FormattedFields<tracing_subscriber::fmt::format::DefaultFields>>()
                    .filter(|f| !f.is_empty())
                {
                    obj["fields"] = serde_json::Value::String(fields.to_string());
                }
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

        if let Ok(mut guard) = self.buffer.lock() {
            let cap = MEMORY_LOG_CAPACITY
                .get()
                .copied()
                .unwrap_or(DEFAULT_MEMORY_LOG_CAPACITY);
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
    fn record_debug(&mut self, field: &Field, value: &dyn stdfmt::Debug) {
        let val = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(val);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(val));
        }
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

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}

// ===========================================================================
//  Virtual target expansion
// ===========================================================================

/// Expands virtual target aliases in an `EnvFilter`-compatible string.
///
/// Currently supported aliases:
/// - `db=<level>` → `sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`
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
            parts.push(format!("sea_orm={level}"));
            parts.push(format!("sea_orm_migration={level}"));
            parts.push(format!("sqlx={level}"));
        } else if directive == "db" {
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
            timer: ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".to_string()),
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

        // Span context
        if let Some(scope) = ctx.event_scope() {
            for span in scope {
                let ext = span.extensions();
                if let Some(fields) = ext.get::<FormattedFields<N>>().filter(|f| !f.is_empty()) {
                    write!(writer, "\n    in {} with {fields}", span.name())?;
                } else {
                    write!(writer, "\n    in {}", span.name())?;
                }
            }
        }

        writeln!(writer)
    }
}

// ===========================================================================
//  Helpers
// ===========================================================================

/// ANSI escape pair `(open, reset)` for the given tracing level.
fn level_ansi(level: tracing::Level) -> (&'static str, &'static str) {
    const RESET: &str = "\x1b[0m";
    match level {
        tracing::Level::ERROR => ("\x1b[31m", RESET),
        tracing::Level::WARN => ("\x1b[33m", RESET),
        tracing::Level::INFO => ("\x1b[32m", RESET),
        tracing::Level::DEBUG => ("\x1b[34m", RESET),
        tracing::Level::TRACE => ("\x1b[35m", RESET),
    }
}
