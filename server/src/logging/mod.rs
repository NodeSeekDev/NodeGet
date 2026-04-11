use std::fmt as stdfmt;

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
    EnvFilter,
};

/// 初始化 tracing 日志系统
///
/// 数据库日志（SeaORM / SQLx）在输出中统一显示为 `target: "db"`。
/// 在 `RUST_LOG` 中也可以直接使用 `db=<level>` 来控制数据库日志级别，
/// 会自动展开为 `sea_orm=<level>,sea_orm_migration=<level>,sqlx=<level>`。
pub fn init() {
    let raw = std::env::var("RUST_LOG").unwrap_or_else(|_| "trace".to_string());
    let expanded = expand_virtual_targets(&raw);
    let env_filter = EnvFilter::new(&expanded);

    let console_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(true)
        .event_format(NodeGetFormat::new());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .init();
}

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

        // Check if this directive is `db=<level>` (or `db` with no level)
        if let Some(level) = directive.strip_prefix("db=") {
            parts.push(format!("sea_orm={level}"));
            parts.push(format!("sea_orm_migration={level}"));
            parts.push(format!("sqlx={level}"));
        } else if directive == "db" {
            // bare `db` without level → expand to bare targets (uses default)
            parts.push("sea_orm".to_string());
            parts.push("sea_orm_migration".to_string());
            parts.push("sqlx".to_string());
        } else {
            parts.push(directive.to_string());
        }
    }

    parts.join(",")
}

// ---------------------------------------------------------------------------
//  Target remapping
// ---------------------------------------------------------------------------

/// Maps known database-related log targets to `"db"`.
///
/// SeaORM emits logs under `sea_orm::*` / `sea_orm_migration::*`,
/// SQLx under `sqlx::*`. All are unified to `"db"` in the output.
fn remap_target(target: &str) -> &str {
    if target.starts_with("sea_orm") || target.starts_with("sqlx") {
        "db"
    } else {
        target
    }
}

// ---------------------------------------------------------------------------
//  Custom event formatter
// ---------------------------------------------------------------------------

/// Custom event format with target remapping.
///
/// Output format (single-line when no span context):
/// ```text
/// 2024-01-15 10:30:00.000  INFO rpc: request received
///     in agent::report_static with uuid=abc-123 token_key=key1 username=user1
/// ```
///
/// Database logs appear as:
/// ```text
/// 2024-01-15 10:30:00.000 DEBUG db: SELECT "agents"."uuid" FROM "agents"
/// ```
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
        // ── Timestamp ───────────────────────────────────────────────
        self.timer.format_time(&mut writer)?;

        // ── Level (with ANSI colour) ────────────────────────────────
        let level = *event.metadata().level();
        if writer.has_ansi_escapes() {
            let (open, close) = level_ansi(level);
            write!(writer, " {open}{level:>5}{close} ")?;
        } else {
            write!(writer, " {level:>5} ")?;
        }

        // ── Target (with remapping) ─────────────────────────────────
        let target = remap_target(event.metadata().target());
        if writer.has_ansi_escapes() {
            // dim style for target
            write!(writer, "\x1b[2m{target}\x1b[0m: ")?;
        } else {
            write!(writer, "{target}: ")?;
        }

        // ── Event fields (message + structured kv) ──────────────────
        ctx.format_fields(writer.by_ref(), event)?;

        // ── Span context (innermost → outermost) ────────────────────
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

// ---------------------------------------------------------------------------
//  Helpers
// ---------------------------------------------------------------------------

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

// Virtual target `db` is expanded to real targets in `expand_virtual_targets()`.
