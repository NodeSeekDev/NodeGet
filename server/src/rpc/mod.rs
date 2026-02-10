use jsonrpsee::RpcModule;
use crate::{rpc, DB};
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::{Value, to_value};
use crate::rpc::nodeget::NodegetServerRpcImpl;

pub mod agent;
pub mod crontab;
// pub mod metadata;
pub mod nodeget;
pub mod task;
pub mod token;
pub mod kv;

pub trait RpcHelper {
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val)
            .map(Set)
            .map_err(|e| NodegetError::SerializationError(format!("Serialization error: {e}")).into())
    }

    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        DB.get()
            .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}

pub fn get_modules() -> RpcModule<NodegetServerRpcImpl> {
    use crate::rpc::agent::RpcServer as AgentRpcServer;
    use crate::rpc::crontab::RpcServer as CrontabRpcServer;
    use crate::rpc::kv::RpcServer as KvRpcServer;
    use crate::rpc::nodeget::RpcServer as NodeGetRpcServer;
    use crate::rpc::task::RpcServer as TaskRpcServer;
    use crate::rpc::token::RpcServer as TokenRpcServer;

    let task_manager = task::TaskManager::global().clone();

    let mut rpc_module = nodeget::NodegetServerRpcImpl.into_rpc();

    rpc_module
        .merge(agent::AgentRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(
            task::TaskRpcImpl {
                manager: task_manager,
            }
                .into_rpc(),
        )
        .unwrap();

    rpc_module
        .merge(token::TokenRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(kv::KvRpcImpl.into_rpc())
        .unwrap();

    rpc_module
        .merge(crontab::CrontabRpcImpl.into_rpc())
        .unwrap();

    rpc_module
}
