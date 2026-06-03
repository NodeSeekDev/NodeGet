#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

//! `ng-db` — `NodeGet` 数据库层
//!
//! 核心职责：
//! - 通过 `SeaORM` 管理 13 张实体表（`entity` 模块）
//! - 提供主库连接的全局单例（`get_db` / `set_db`），服务端启动时初始化
//! - `DbRegistryManager` 管理用户创建的 `SQLite` 数据库池，含过期清理与自动种子恢复
//! - SQL 辅助工具：`row_to_json`、`json_to_sea_value`、`is_read_query`
//! - 数据库名称校验：`validate_db_name`
//!
//! 协作关系：
//! - 服务端二进制在启动时调用 `set_db` 和 `DbRegistryManager::init`
//! - `ng-infra`、`ng-token` 等业务 Crate 通过 `get_db()` 获取主库连接
//! - RPC 层（`db` / `nodeget` 命名空间）通过 `rpc_exec!` 宏统一日志输出
//!
//! Feature Gate：
//! - 默认仅导出 `entity` 模块，供 Agent 等轻量依赖使用
//! - `server` feature 启用连接初始化、DbRegistry、RPC 等服务端专属功能

pub mod entity;

// ── 主库全局单例 ──────────────────────────────────────────────────

/// 全局数据库连接，`ManuallyDrop` 包裹以避免进程退出时的昂贵析构。
///
/// `PostgreSQL` 连接池的 `drop` 需要 join 后台维护任务，耗时可达数秒。
/// 进程退出时 OS 回收所有资源（TCP 连接、内存），无需显式析构。
/// 若后续需要优雅关闭池，可在 `serve.rs` 退出前调用 `take_and_close_db()`。
static DB: std::sync::OnceLock<std::mem::ManuallyDrop<sea_orm::DatabaseConnection>> =
    std::sync::OnceLock::new();

/// 获取全局主库连接
///
/// - 返回值：若已初始化则返回 `Some(&DatabaseConnection)`，否则 `None`
/// - 服务端各模块通过此函数共享同一个数据库连接
pub fn get_db() -> Option<&'static sea_orm::DatabaseConnection> {
    use std::ops::Deref;
    DB.get().map(Deref::deref)
}

/// 设置全局主库连接，仅应在服务端启动时调用一次
///
/// - `conn` — `SeaORM` 数据库连接实例
/// - 若重复调用，新连接会被丢弃并输出警告日志（OnceLock 语义）
pub fn set_db(conn: sea_orm::DatabaseConnection) {
    if DB.set(std::mem::ManuallyDrop::new(conn)).is_err() {
        tracing::warn!(target: "db", "set_db called twice; new connection discarded (OnceLock already set)");
    }
}

/// 取出并优雅关闭全局数据库连接（可选，仅用于需要显式 `drop` 的场景）
///
/// 调用后 `get_db()` 返回 `None`。若从未调用此函数，连接在进程退出时由 OS 回收。
///
/// # Safety
///
/// 必须确保无其他代码仍在使用 `get_db()` 返回的引用。
#[allow(dead_code)]
pub unsafe fn take_and_close_db() {
    // SAFETY: 调用者保证无其他引用在使用中
    let db_ptr: *mut std::sync::OnceLock<std::mem::ManuallyDrop<sea_orm::DatabaseConnection>> =
        &DB as *const _ as *mut _;
    // SAFETY: 调用者保证独占访问
    if let Some(md) = unsafe { (*db_ptr).take() } {
        // ManuallyDrop::into_inner 恢复所有权并执行正常 drop
        drop(std::mem::ManuallyDrop::into_inner(md));
    }
}

// ── 服务端专属模块 ────────────────────────────────────────────────

#[cfg(feature = "server")]
pub mod db_connection;
#[cfg(feature = "server")]
pub mod db_registry;
#[cfg(feature = "server")]
pub mod rpc;

// ── 便捷 Re-export ────────────────────────────────────────────────

#[cfg(feature = "server")]
pub use db_connection::{DbConnectionConfig, init_db_connection};
#[cfg(feature = "server")]
pub use db_registry::{
    DbExecResult, DbInfo, DbRegistryManager, is_read_query, json_to_sea_value, row_to_json,
};
#[cfg(feature = "server")]
pub use rpc::db::auth::validate_db_name;
