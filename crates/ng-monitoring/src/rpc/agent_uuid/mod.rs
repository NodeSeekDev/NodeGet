//! Agent-UUID RPC namespace
//!
//! 提供针对 `monitoring_uuid`（权威 Agent 表）的面向前端操作：
//! - `agent-uuid.list_all`                     — 列出所有非软删除的 Agent UUID
//! - `agent-uuid.list_all_with_agent_mode`     — 列出所有 Agent UUID（包含软删除状态）
//! - `agent-uuid.delete`                       — 按 UUID 软删除 Agent

mod delete;
mod list_all;
mod list_all_with_agent_mode;

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::token_identity;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

#[rpc(server, namespace = "agent-uuid")]
pub trait AgentUuidRpc {
    #[method(name = "list_all")]
    async fn list_all_agent_uuids(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "list_all_with_agent_mode")]
    async fn list_all_agent_uuids_with_agent_mode(&self, token: String)
    -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete_agent_uuid(&self, token: String, agent_uuid: Uuid) -> RpcResult<Box<RawValue>>;
}

pub struct AgentUuidRpcImpl;

impl ng_infra::server::RpcHelper for AgentUuidRpcImpl {}

#[jsonrpsee::core::async_trait]
impl AgentUuidRpcServer for AgentUuidRpcImpl {
    async fn list_all_agent_uuids(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "agent-uuid::list_all", token_key = tk, username = un);
        async { rpc_exec!(list_all::list_all_agent_uuids(token).await) }
            .instrument(span)
            .await
    }

    async fn list_all_agent_uuids_with_agent_mode(
        &self,
        token: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "agent-uuid::list_all_with_agent_mode", token_key = tk, username = un);
        async {
            rpc_exec!(list_all_with_agent_mode::list_all_agent_uuids_with_agent_mode(token).await)
        }
        .instrument(span)
        .await
    }

    async fn delete_agent_uuid(&self, token: String, agent_uuid: Uuid) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "agent-uuid::delete", agent_uuid = %agent_uuid, token_key = tk, username = un);
        async { rpc_exec!(delete::delete_agent_uuid(token, agent_uuid).await) }
            .instrument(span)
            .await
    }
}
