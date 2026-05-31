#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

//! `ng-db` — Database layer for `NodeGet`.
//!
//! This crate provides:
//! - `SeaORM` entities (11 tables)
//! - Database connection initialization (server feature)
//! - DB global singleton (`get_db` / `set_db`)
//! - `DbRegistryManager` for user-created `SQLite` databases
//! - SQL helpers (`row_to_json`, `json_to_sea_value`, `is_read_query`)
//! - `validate_db_name` / `ValidDbName` (implements `NameValidator`)
//! - `db` RPC namespace (server feature)
//! - `nodeget-server::database_storage`, `exec_sql`, `get_database_type` (server feature)

pub mod entity;

// ── DB global singleton ─────────────────────────────────────────────

static DB: std::sync::OnceLock<sea_orm::DatabaseConnection> = std::sync::OnceLock::new();

/// Get the global database connection, if initialized.
pub fn get_db() -> Option<&'static sea_orm::DatabaseConnection> {
    DB.get()
}

/// Set the global database connection. Called once during server startup.
pub fn set_db(conn: sea_orm::DatabaseConnection) {
    let _ = DB.set(conn);
}

// ── Server-only modules ─────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod db_connection;
#[cfg(feature = "server")]
pub mod db_registry;
#[cfg(feature = "server")]
pub mod rpc;

// ── Re-exports for convenience ──────────────────────────────────────

#[cfg(feature = "server")]
pub use db_connection::{DbConnectionConfig, init_db_connection};
#[cfg(feature = "server")]
pub use db_registry::{
    DbExecResult, DbInfo, DbRegistryManager, is_read_query, json_to_sea_value, row_to_json,
};
#[cfg(feature = "server")]
pub use rpc::db::auth::{ValidDbName, validate_db_name};
