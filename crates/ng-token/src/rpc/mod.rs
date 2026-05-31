use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use ng_core::permission::create::TokenCreationRequest;
use ng_core::permission::data_structure::Limit;
use ng_infra::rpc_exec;
use ng_infra::server::token_identity;
use serde_json::value::RawValue;
use tracing::Instrument;

mod change_password;
mod create;
mod delete;
mod edit;
mod get;
mod list_all_tokens;
mod roll_token_secret;
mod utils;

#[rpc(server, namespace = "token")]
pub trait Rpc {
    #[method(name = "get")]
    async fn get(&self, token: String, supertoken: Option<String>) -> RpcResult<Box<RawValue>>;

    #[method(name = "create")]
    async fn create(
        &self,
        father_token: String,
        token_creation: TokenCreationRequest,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, target_token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "change_password")]
    async fn change_password(
        &self,
        token: String,
        target_token: String,
        new_password: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "roll_token_secret")]
    async fn roll_token_secret(
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

#[jsonrpsee::core::async_trait]
impl RpcServer for TokenRpcImpl {
    async fn get(&self, token: String, supertoken: Option<String>) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "token", "token::get", token_key = tk, username = un, has_supertoken = supertoken.is_some());
        async { rpc_exec!(get::get(token, supertoken).await) }
            .instrument(span)
            .await
    }

    async fn create(
        &self,
        father_token: String,
        token_creation: TokenCreationRequest,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&father_token);
        let span = tracing::info_span!(target: "token", "token::create", token_key = tk, username = un, target_username = ?token_creation.username);
        async { rpc_exec!(create::create(father_token, token_creation).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, target_token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let (target_tk, target_un) = token_identity(&target_token);
        let span = tracing::info_span!(target: "token", "token::delete", token_key = tk, username = un, target_token_key = target_tk, target_username = target_un);
        async { rpc_exec!(delete::delete(token, target_token).await) }
            .instrument(span)
            .await
    }

    async fn change_password(
        &self,
        token: String,
        target_token: String,
        new_password: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let (target_tk, target_un) = token_identity(&target_token);
        let span = tracing::info_span!(
            target: "token",
            "token::change_password",
            token_key = tk,
            username = un,
            target_token_key = target_tk,
            target_username = target_un,
        );
        async {
            rpc_exec!(change_password::change_password(token, target_token, new_password).await)
        }
        .instrument(span)
        .await
    }

    async fn roll_token_secret(
        &self,
        token: String,
        target_token: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let (target_tk, target_un) = token_identity(&target_token);
        let span = tracing::info_span!(
            target: "token",
            "token::roll_token_secret",
            token_key = tk,
            username = un,
            target_token_key = target_tk,
            target_username = target_un,
        );
        async { rpc_exec!(roll_token_secret::roll_token_secret(token, target_token).await) }
            .instrument(span)
            .await
    }

    async fn list_all_tokens(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "token", "token::list_all_tokens", token_key = tk, username = un);
        async { rpc_exec!(list_all_tokens::list_all_tokens(token).await) }
            .instrument(span)
            .await
    }

    async fn edit(
        &self,
        token: String,
        target_token: String,
        limit: Vec<Limit>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let (target_tk, target_un) = token_identity(&target_token);
        let span = tracing::info_span!(target: "token", "token::edit", token_key = tk, username = un, target_token_key = target_tk, target_username = target_un);
        async { rpc_exec!(edit::edit(token, target_token, limit).await) }
            .instrument(span)
            .await
    }
}
