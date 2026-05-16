//! Agent-UUID RPC namespace
//!
//! 提供针对 `monitoring_uuid`（权威 Agent 表）的面向前端操作：
//! - `agent-uuid.list_all` — 列出所有非软删除的 Agent UUID
//! - `agent-uuid.delete`    — 按 UUID 软删除 Agent

mod delete;
mod list_all;

use crate::rpc::RpcHelper;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::RpcResult;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

#[rpc(server)]
pub trait AgentUuidRpc {
    #[method(name = "agent-uuid.list_all")]
    async fn list_all_agent_uuids(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "agent-uuid.delete")]
    async fn delete_agent_uuid(
        &self,
        token: String,
        agent_uuid: Uuid,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct AgentUuidRpcImpl;

impl RpcHelper for AgentUuidRpcImpl {}

#[jsonrpsee::core::async_trait]
impl AgentUuidRpcServer for AgentUuidRpcImpl {
    async fn list_all_agent_uuids(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = crate::rpc::token_identity(&token);
        let span = tracing::info_span!(target: "server", "agent-uuid::list_all", token_key = tk, username = un);
        async { crate::rpc::rpc_exec!(list_all::list_all_agent_uuids(token).await) }
            .instrument(span)
            .await
    }

    async fn delete_agent_uuid(&self, token: String, agent_uuid: Uuid) -> RpcResult<Box<RawValue>> {
        let (tk, un) = crate::rpc::token_identity(&token);
        let span = tracing::info_span!(target: "server", "agent-uuid::delete", agent_uuid = %agent_uuid, token_key = tk, username = un);
        async { crate::rpc::rpc_exec!(delete::delete_agent_uuid(token, agent_uuid).await) }
            .instrument(span)
            .await
    }
}
