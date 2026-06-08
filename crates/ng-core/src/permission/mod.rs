//! 权限与 Token 模型定义
//!
//! 包含 RBAC 权限体系的核心数据结构（Scope、Permission、Limit、Token）、
//! Token 创建请求体以及 Token/Auth 双模式认证类型。
//! 所有业务 crate 的鉴权逻辑均引用此模块中的类型。

pub mod create;
pub mod data_structure;
#[cfg(feature = "for-server")]
pub mod permission_checker;
pub mod token_auth;
