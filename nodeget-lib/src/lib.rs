#![feature(duration_millis_float)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code
)]

// 配置管理模块，处理 Agent 和 Server 的配置
pub mod config;
// 监控数据模块，包含监控数据的结构和查询功能
pub mod monitoring;

// 权限管理模块，仅在启用 for-server 特性时编译
#[cfg(feature = "for-server")]
pub mod permission;

// 任务管理模块，处理各种任务类型的定义和执行
pub mod task;

// 工具函数模块，包含通用的辅助函数
pub mod utils;
pub mod metadata;
