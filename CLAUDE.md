# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
# Build all crates
cargo build

# Build specific crate (agent, server, nodeget-lib, migration)
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
```

No test suite exists yet. No Makefile.

## Workspace Structure

```
NodeGet/
├── server/           # JSON-RPC + WebSocket server (axum + jsonrpsee)
│   ├── src/rpc/      #   13 RPC namespaces, each with auth/query/mutation files
│   ├── src/entity/   #   SeaORM entities (11 tables)
│   ├── src/js_runtime/ # QuickJS sandbox (rquickjs) with runtime pool
│   ├── src/crontab/  #   Cron scheduler (per-minute tick)
│   ├── src/token/    #   Token auth with super-token + RBAC limits
│   ├── src/cache/    #   Generic DB-backed memory cache framework
│   └── migration/    #   SeaORM migrations (17 steps)
├── agent/            # Monitoring agent deployed on target servers
│   ├── src/monitoring/ # System/GPU/disk/network data collectors
│   ├── src/tasks/    #   Task executors (ping, DNS, HTTP, shell, etc.)
│   └── src/rpc/      #   Multi-server WebSocket connection manager
└── nodeget-lib/      # Shared library (data structures, config, permissions)
    ├── src/monitoring/ # Data structures + query DSL for monitoring
    ├── src/permission/ # RBAC model (Scope + Permission + Limit)
    ├── src/config/   #   Server/Agent config parsing
    └── src/task/     #   Task type definitions + query DSL
```

## Architecture

**Communication**: WebSocket + JSON-RPC 2.0. Server exposes HTTP endpoints at `/` and `/nodeget/rpc`. Agent connects as WebSocket client.

**Database**: PostgreSQL or SQLite via SeaORM. Connection is a global singleton (`server::DB`). SQLite auto-enables WAL mode.

**Config hot-reload**: Both server and agent watch for `RELOAD_NOTIFY` signal. Server re-reads config file; agent receives `EditConfig` task then restarts runtime tasks.

**Agent multi-server**: One agent connects to N servers simultaneously. Each server gets an independent `connection_manager` coroutine with exponential-backoff reconnect.

### Data Flow

1. Agent collects monitoring data on configurable intervals (static 5min, dynamic/summary 1s default)
2. Data flows through mpsc channels → `MonitoringBuffer` → batch INSERT to DB
3. In-memory caches (`MonitoringLastCache`, `StaticHashCache`, `MonitoringUuidCache`) serve queries without hitting DB
4. Tasks flow: Server RPC → `TaskManager` → broadcast channel → Agent subscription → execute → upload result

### Caching Pattern

All "load-all-from-DB" caches use `server::cache::DbBackedCache` trait + `make_global_cache!` macro. This generates a `OnceLock` global singleton with `init()` / `global()` / `reload()` methods. Used by: Token, Crontab, StaticBucket, MonitoringUUID, MonitoringLast, StaticHash.

### JS Worker System

QuickJS runtime pool: each registered script gets its own OS thread + `QuickJS` instance. Communication via channels (`Execute`/`Shutdown` commands). Bytecode caching avoids recompilation. OS thread watchdog enforces hard timeout (kills CPU-bound loops). Built-in APIs injected: `nodeget()` for internal RPC, `execSql()`, `db.*`, `fetch`, `randomUUID()`.

### RBAC Permission Model

Every RPC method authenticates via `TokenOrAuth` (key:secret token OR username|password). Tokens carry a `Vec<Limit>` specifying scope+permission constraints. Super-token (id=1, constant-time comparison) bypasses all limits. Token auth uses SHA256 with "NODEGET" salt.

## Key Conventions

- **Edition 2024** — uses Rust 2024 edition features
- **Clippy strict** — workspace is compiled with `clippy::all`, `clippy::pedantic`, `clippy::nursery`; cast lints are suppressed globally
- **Chinese comments** — inline comments and config examples are in Chinese; keep consistent
- **JSON-RPC response format** — all RPC methods return `RpcResult<Box<RawValue>>` via the `rpc_exec!` macro for uniform logging
- **Entity generation** — run `server/generate_entity.sh` after migration changes to regenerate SeaORM entities
- **Config format** — TOML; agent config uses `[[server]]` array-of-tables for multi-server; server config uses `[database]`, `[logging]`, `[monitoring_buffer]` sections
- **Soft delete** — `monitoring_uuid` table uses `soft_delete` flag instead of actual deletion; UUID cache auto-resurrects soft-deleted entries on `get_or_insert`
- **Path safety** — static file operations use `validate_name`, `validate_sub_path`, `resolve_safe_file_path` to prevent traversal attacks; same discipline required for any new path-handling code
