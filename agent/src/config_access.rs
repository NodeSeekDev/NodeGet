//! Shared accessor for the process-wide [`AgentConfig`].
//!
//! 历史上 `rpc/mod.rs`、`tasks/mod.rs`、`tasks/execute.rs` 三个模块各自写了一份
//! `AGENT_CONFIG.get()…read()…clone()` 的包装函数，命名分别为
//! `get_agent_config_safe` / `get_agent_config`，逻辑完全相同但错误类型有差别，
//! 维护成本高（`review_agent.md` #61）。这里集中一份实现，以 [`NodegetError`] 作为
//! 基础错误类型，上层若需要 [`anyhow::Error`] 只要 `.map_err(Into::into)`。
//!
//! 另外对"已经通过 `get_agent_config` 间接走通、只需要 `agent_uuid` 的热路径"提供一个
//! panic-on-invariant-violation 的简化辅助 [`current_agent_uuid_string`]（`review_agent.md` 低优）
//! ，替代散落在各模块的 `.expect("Agent config not initialized") / .expect("... poisoned")`。

use crate::AGENT_CONFIG;
use ng_config::config::agent::AgentConfig;
use ng_core::error::NodegetError;

/// Return a cloned snapshot of the current [`AgentConfig`].
///
/// # Errors
///
/// * 若 [`crate::AGENT_CONFIG`] 尚未初始化（理论上只在启动阶段发生）。
/// * 若 `RwLock` 被毒化（上一个写者在写期间 panic）。
pub fn get_agent_config() -> Result<AgentConfig, NodegetError> {
    AGENT_CONFIG
        .get()
        .ok_or_else(|| NodegetError::Other("Agent config not initialized".to_owned()))?
        .read()
        .map(|guard| guard.clone())
        .map_err(|_| NodegetError::Other("AGENT_CONFIG lock poisoned".to_owned()))
}

/// Return the agent uuid as a [`uuid::Uuid`].
///
/// # Panics
///
/// 同 `current_agent_uuid_string`。
#[must_use]
pub fn current_agent_uuid() -> uuid::Uuid {
    current_agent_uuid_string()
        .parse()
        .expect("agent_uuid in config is not a valid UUID")
}
///
/// # Panics
///
/// 在 `AGENT_CONFIG` 未初始化或 `RwLock` 毒化时直接 panic。这是一个热路径辅助：
/// 所有使用该函数的代码都是"没拿到 uuid 就无法继续运行"的架构前提，散写
/// `.expect()` 反而更难维护（`review_agent.md` 低优）。
#[must_use]
pub fn current_agent_uuid_string() -> String {
    AGENT_CONFIG
        .get()
        .expect("Agent config not initialized")
        .read()
        .expect("AGENT_CONFIG lock poisoned")
        .agent_uuid
        .to_string()
}
