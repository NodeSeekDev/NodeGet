use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use serde_json::Value;
use uuid::Uuid;
use nodeget_lib::metadata;
use nodeget_lib::permission::data_structure::{Metadata, Permission, Scope, Task};
use nodeget_lib::permission::token_auth::TokenOrAuth;
use crate::rpc::RpcHelper;
use crate::rpc::task::TaskRpcImpl;
use crate::token::get::check_token_limit;

// NodeGet 服务端基础功能 RPC 接口定义
#[rpc(server, namespace = "metadata")]
pub trait Rpc {
    #[method(name = "get")]
    async fn get(&self, token: String, uuid: Uuid) -> metadata::Metadata;

    #[method(name = "write")]
    async fn write(&self, token: String, metadata: metadata::Metadata) -> Value;
}

pub struct MetadataRpcImpl;

#[async_trait]
impl RpcServer for MetadataRpcImpl {
    async fn get(&self, token: String, uuid: Uuid) -> metadata::Metadata {
        let process_logic = async {
            let token_or_auth = match TokenOrAuth::from_full_token(&token) {
                Ok(toa) => {
                    toa
                }
                Err(e) => {
                    return Err((101, format!("Failed to parse token: {e}")))
                }
            };

            let is_allowed = check_token_limit(
                &token_or_auth,
                vec![Scope::AgentUuid(uuid)],
                vec![Permission::Metadata(Metadata::Read)],
            )
                .await?;

            if !is_allowed {
                return Err((
                    102,
                    format!(
                        "Permission Denied: Missing Task Create ({task_name}) permission for this Agent"
                    ),
                ));
            }


            // 查询
            let db = TaskRpcImpl::get_db()?;



            todo!()
        };

        todo!()
    }
}
