//! ng-core：NodeGet 工作区的基础类型库
//!
//! 提供跨 crate 共享的错误定义、权限模型、监控数据结构、
//! JS Worker 结果查询、自更新逻辑及通用工具函数。
//! Server 与 Agent 均依赖此 crate；通过 `for-server` / `for-agent`
//! feature gate 控制各自可见的符号。

pub mod config;
pub mod error;
pub mod js_result;
pub mod monitoring;
pub mod permission;
pub mod self_update;
pub mod utils;

/// 名称校验 trait，为需要验证输入名称的类型提供统一接口。
///
/// - 实现类型通过 `Self::validate(name)` 创建实例
/// - 校验失败返回 `NodegetError::InvalidInput`
pub trait NameValidator: Sized {
    /// 校验给定名称并构造自身。
    ///
    /// - `name`：待校验的名称字符串
    /// - 返回校验通过后的 Self，或校验失败的错误
    fn validate(name: &str) -> Result<Self, error::NodegetError>;
}
