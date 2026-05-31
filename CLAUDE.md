# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
# Build all crates
cargo build

# Build specific crate
cargo build --package nodeget-server
cargo build --package nodeget-agent

# Release build (optimized)
cargo build --release

# Minimal size build (Docker uses this profile)
cargo build --package nodeget-server --profile minimal

# Run server (needs config.toml)
cargo run --package nodeget-server -- serve -c config.toml

# Run agent
cargo run --package nodeget-agent -- -c config.toml

# Lint
cargo clippy --workspace

# Check without building
cargo check --workspace

# Run tests
cargo test --workspace
```

## Workspace Structure

```
NodeGet/
├── server/                # Thin server binary (main, logging, subcommands, rpc_nodeget, rpc_timing)
│   └── generate_entity.sh  # SeaORM entity generator (NOTE: output path needs update to crates/ng-db/src/entity)
├── agent/                 # Monitoring agent binary (monitoring, tasks, multi-server RPC)
├── crates/
│   ├── ng-core/           # Errors, version, utils, NameValidator, Token/Scope/Permission/Limit/TokenOrAuth
│   ├── ng-db/             # Entities (13 tables), DB connection global, DbRegistry, db RPC
│   │   └── migration/     #   SeaORM migrations (17 steps)
│   ├── ng-infra/          # DbBackedCache + make_global_cache!, AuthChecker, rpc_exec!, RpcHelper, token_identity
│   ├── ng-config/         # ServerConfig, AgentConfig, CLI args, global config, read/edit_config RPC
│   ├── ng-monitoring/     # Monitoring data structures, caches (UUID/Last/StaticHash), buffer, agent/agent-uuid RPC
│   ├── ng-token/          # TokenCache, super-token, token generation/verification, token RPC
│   ├── ng-kv/             # KV store types, namespace management, kv RPC
│   ├── ng-task/           # Task types, TaskManager, task dispatch, task RPC
│   ├── ng-crontab/        # Cron types, CrontabCache, scheduler, crontab/crontab-result RPC
│   ├── ng-js-runtime/     # QuickJS pool, watchdog, bytecode cache, JsWorkerService trait
│   ├── ng-js-worker/      # Worker CRUD, execution service, js-worker/js-result RPC
│   ├── ng-static/         # Static file cache, upload/download/WebDAV, static-bucket/static-bucket-file RPC
│   └── ng-terminal/       # WebSocket terminal proxy, session management
└── refactor-modular-crates.md  # Refactoring plan document (partially outdated — see docs-vs-code-report.md)
```

## Architecture

**Communication**: WebSocket + JSON-RPC 2.0. Server exposes HTTP at `/` and `/nodeget/rpc`. Agent connects as WebSocket client. Custom jsonrpsee fork (`infinitefield/jsonrpsee`) uses `_` as namespace separator (not `.`).

**Database**: PostgreSQL or SQLite via SeaORM. Global singleton via `ng_db::get_db() -> Option<&'static DatabaseConnection>`. SQLite auto-enables WAL mode.

**Config hot-reload**: Both server and agent watch for `RELOAD_NOTIFY` signal (via `ng_config`). Server re-reads config file; agent receives `EditConfig` task then restarts runtime tasks.

**Agent multi-server**: One agent connects to N servers simultaneously. Each server gets an independent `connection_manager` coroutine with exponential-backoff reconnect.

### Data Flow

1. Agent collects monitoring data on configurable intervals (static 5min, dynamic/summary 1s default)
2. Data flows through mpsc channels → `MonitoringBuffer` → batch INSERT to DB
3. In-memory caches (`MonitoringLastCache`, `StaticHashCache`, `MonitoringUuidCache`) serve queries without hitting DB
4. Tasks flow: Server RPC → `TaskManager` → broadcast channel → Agent subscription → execute → upload result

### RPC Namespace Composition

Server binary assembles all RPC namespaces via `build_modules()` in `server/src/rpc_nodeget.rs`, merging `RpcModule`s from 8 crates:

| Namespace | Provider Crate | RPC Methods |
|-----------|---------------|-------------|
| `nodeget-server` | server + ng-monitoring | hello, version, uuid, read_config, edit_config, database_storage, exec_sql, get_database_type, log, stream_log, self_update, list_all_agent_uuid |
| `agent` | ng-monitoring | Agent monitoring queries |
| `agent-uuid` | ng-monitoring | Agent UUID management |
| `task` | ng-task | Task dispatch and results |
| `token` | ng-token | Token CRUD and verification |
| `kv` | ng-kv | KV store operations |
| `db` | ng-db | Database registry |
| `js-worker` | ng-js-worker | JS worker CRUD and execution |
| `js-result` | ng-js-worker | JS execution results |
| `crontab` | ng-crontab | Cron job management |
| `crontab-result` | ng-crontab | Cron execution results |
| `static-bucket` | ng-static | Static bucket CRUD |
| `static-bucket-file` | ng-static | Static file operations |

All RPC methods return `RpcResult<Box<RawValue>>` via the `rpc_exec!` macro for uniform logging.

### Caching Pattern

All "load-all-from-DB" caches use `ng_infra::server::DbBackedCache` trait + `ng_infra::server::make_global_cache!` macro (ng-infra's `server` feature). Generates a `OnceLock` global singleton with `init()` / `global()` / `reload()` methods. Used by: TokenCache, CrontabCache, StaticCache, MonitoringUuidCache, MonitoringLastCache, StaticHashCache.

### Trait Injection Pattern

Business crates use OnceLock-based trait injection to break circular dependencies. Server binary registers concrete implementations at startup in `serve.rs`:

| Injected Trait | Defining Crate | Methods | Server Implementation |
|---------------|---------------|---------|----------------------|
| `AuthChecker` | ng-infra | `check(raw_token) → Token` | `TokenAuthChecker` (ng-token) |
| `AuthProvider` | ng-db | `check_token_limit`, `check_super_token` | `ServerAuthProvider` |
| `TokenPermissionChecker` | ng-kv, ng-static, ng-js-worker, ng-terminal | `check_token_limit`, `check_super_token`, (optional) `get_token` | Per-crate checker structs |
| `TaskAuthProvider` | ng-task | `check_token_limit`, `check_super_token`, `get_token` | `TaskAuthProvider` |
| `CheckSuperTokenFn` | ng-config | `check_super_token` (fn pointer) | Closure → `ng_token::check_super_token` |
| `JsWorkerService` | ng-js-runtime | `run_inline_call_and_record_result`, `get_rpc_module` | `JsWorkerServiceImpl` |
| `JsWorkerScheduler` | ng-crontab | `enqueue_run` | `CronJsWorkerScheduler` |
| `MonitoringUuidProvider` | ng-task | `get_or_insert`, `reload` | `TaskMonitoringUuidProvider` |

All implementations ultimately delegate to `ng_token` functions.

### JS Worker System

QuickJS runtime pool (ng-js-runtime): each registered script gets its own OS thread + QuickJS instance. Communication via channels (`Execute`/`Shutdown`). Bytecode caching avoids recompilation. OS thread watchdog enforces hard timeout (kills CPU-bound loops). Built-in APIs: `nodeget()` for internal RPC, `execSql()`, `db.*`, `fetch`, `randomUUID()`.

ng-js-worker provides CRUD, execution service, and auth-gated RPC on top of the runtime pool.

### Feature Gate Pattern

All business crates use a uniform feature pattern:

- **`default = []`**: Only types, data structures, query DSL — agent can safely depend
- **`server` feature**: Adds RPC handlers, DB queries, caches, buffer — only server binary enables

Exception: `ng-core` uses `for-server` / `for-agent` features instead (brings in `libc`).

Agent depends on `ng-core/for-agent`, `ng-config`, `ng-task`, `ng-monitoring` — none with `server` feature.

### HTTP Routes (non-RPC)

| Path | Handler | Source |
|------|---------|--------|
| `/`, `/nodeget/rpc` | JSON-RPC + WebSocket + landing | server binary |
| `/nodeget/static/*` + WebDAV | Static file service | `ng_static::router::router()` |
| `/worker-route/{name}/*` | JS worker HTTP routes | server binary inline |
| `/terminal` | Terminal WebSocket | `ng_terminal::router()` |
| `.fallback()` | WS upgrade / static root / JSON-RPC | server binary |

### RBAC Permission Model

Every RPC method authenticates via `TokenOrAuth` (key:secret token OR username|password). Tokens carry a `Vec<Limit>` specifying scope+permission constraints. Super-token (id=1, constant-time comparison) bypasses all limits. Token auth uses SHA256 with "NODEGET" salt.

## Key Conventions

- **Edition 2024** — uses Rust 2024 edition features
- **Clippy strict** — workspace compiled with `clippy::all`, `clippy::pedantic`, `clippy::nursery`; cast lints suppressed globally
- **Chinese comments** — inline comments and config examples are in Chinese; keep consistent
- **Custom jsonrpsee fork** — `infinitefield/jsonrpsee`, namespace separator is `_` not `.`
- **`#[rpc]` proc macro only** — never use manual `register_method`/`register_async_method`; always use `#[rpc(server, namespace = "...")]` + `#[method(name = "...")]`
- **Entity generation** — run `server/generate_entity.sh` after migration changes (NOTE: output path `-o` should point to `../crates/ng-db/src/entity`)
- **Config format** — TOML; agent config uses `[[server]]` array-of-tables for multi-server; server config uses `[database]`, `[logging]`, `[monitoring_buffer]` sections
- **Soft delete** — `monitoring_uuid` table uses `soft_delete` flag instead of actual deletion; UUID cache auto-resurrects soft-deleted entries on `get_or_insert`
- **Path safety** — static file operations use `validate_name`, `validate_sub_path`, `resolve_safe_file_path` to prevent traversal attacks; same discipline required for any new path-handling code
