mod get;
mod create;

use crate::token::generate_token::generate_and_store_token;
use crate::token::get::get_token;
use crate::token::split_username_password;
use jsonrpsee::proc_macros::rpc;
use log::debug;
use migration::async_trait::async_trait;
use nodeget_lib::monitoring::data_structure::{DynamicMonitoringData, StaticMonitoringData};
use nodeget_lib::permission::create::TokenCreationRequest;
use nodeget_lib::utils::error_message::generate_error_message;
use serde_json::{Value, json};

#[rpc(server, namespace = "token")]
pub trait Rpc {
    #[method(name = "get")]
    async fn get(&self, token: String) -> Value;

    #[method(name = "create")]
    async fn create(&self, father_token: String, token_creation: TokenCreationRequest) -> Value;
}
pub struct TokenRpcImpl;

#[async_trait]
impl RpcServer for TokenRpcImpl {
    async fn get(&self, token: String) -> Value {
        get::get(token).await
    }

    async fn create(&self, father_token: String, token_creation: TokenCreationRequest) -> Value {
        create::create(father_token, token_creation).await
    }
}
