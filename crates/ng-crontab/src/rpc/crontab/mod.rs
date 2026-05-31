use crate::CronType;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use ng_infra::rpc_exec;
use ng_infra::server::{RpcHelper, token_identity};
use serde_json::value::RawValue;
use tracing::Instrument;

mod auth;
mod create;
mod delete;
mod edit;
mod get;
mod set_enable;

#[rpc(server, namespace = "crontab")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "edit")]
    async fn edit(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "get")]
    async fn get(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "set_enable")]
    async fn set_enable(
        &self,
        token: String,
        name: String,
        enable: bool,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct CrontabRpcImpl;

impl RpcHelper for CrontabRpcImpl {}

#[async_trait]
impl RpcServer for CrontabRpcImpl {
    async fn create(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::create", token_key = tk, username = un, name = %name, cron_expression = %cron_expression, cron_type = ?cron_type);
        async { rpc_exec!(create::create(token, name, cron_expression, cron_type).await) }
            .instrument(span)
            .await
    }

    async fn edit(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::edit", token_key = tk, username = un, name = %name, cron_expression = %cron_expression, cron_type = ?cron_type);
        async { rpc_exec!(edit::edit(token, name, cron_expression, cron_type).await) }
            .instrument(span)
            .await
    }

    async fn get(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span =
            tracing::info_span!(target: "crontab", "crontab::get", token_key = tk, username = un);
        async { rpc_exec!(get::get(token).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    async fn set_enable(
        &self,
        token: String,
        name: String,
        enable: bool,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "crontab", "crontab::set_enable", token_key = tk, username = un, name = %name, enable = enable);
        async { rpc_exec!(set_enable::set_enable(token, name, enable).await) }
            .instrument(span)
            .await
    }
}

/// Build and return the `crontab` RPC module.
pub fn rpc_module() -> jsonrpsee::RpcModule<CrontabRpcImpl> {
    CrontabRpcImpl.into_rpc()
}
