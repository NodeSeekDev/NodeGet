pub mod list_all_agent_uuid;

use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use ng_db::rpc_exec;
use serde_json::value::RawValue;
use tracing::Instrument;

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "list_all_agent_uuid")]
    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>>;
}

pub struct NodegetServerRpcImpl;

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        let span = tracing::info_span!(target: "server", "nodeget-server::list_all_agent_uuid");
        async { rpc_exec!(list_all_agent_uuid::list_all_agent_uuid(token).await) }
            .instrument(span)
            .await
    }
}
