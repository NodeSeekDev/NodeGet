use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use migration::async_trait::async_trait;
use nodeget_lib::permission::data_structure::Limit;
use nodeget_lib::permission::create::TokenCreationRequest;
use serde_json::value::RawValue;

mod create;
mod delete;
mod edit;
mod get;
mod list_all_tokens;

#[rpc(server, namespace = "token")]
pub trait Rpc {
    #[method(name = "get")]
    async fn get(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "create")]
    async fn create(
        &self,
        father_token: String,
        token_creation: TokenCreationRequest,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(
        &self,
        token: String,
        target_token: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "list_all_tokens")]
    async fn list_all_tokens(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "edit")]
    async fn edit(
        &self,
        token: String,
        target_token: String,
        limit: Vec<Limit>,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct TokenRpcImpl;

#[async_trait]
impl RpcServer for TokenRpcImpl {
    async fn get(&self, token: String) -> RpcResult<Box<RawValue>> {
        get::get(token).await
    }

    async fn create(
        &self,
        father_token: String,
        token_creation: TokenCreationRequest,
    ) -> RpcResult<Box<RawValue>> {
        create::create(father_token, token_creation).await
    }

    async fn delete(
        &self,
        token: String,
        target_token: String,
    ) -> RpcResult<Box<RawValue>> {
        delete::delete(token, target_token).await
    }

    async fn list_all_tokens(&self, token: String) -> RpcResult<Box<RawValue>> {
        list_all_tokens::list_all_tokens(token).await
    }

    async fn edit(
        &self,
        token: String,
        target_token: String,
        limit: Vec<Limit>,
    ) -> RpcResult<Box<RawValue>> {
        edit::edit(token, target_token, limit).await
    }
}
