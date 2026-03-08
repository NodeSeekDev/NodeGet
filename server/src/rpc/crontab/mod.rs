use crate::rpc::RpcHelper;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::crontab::CronType;
use serde_json::value::RawValue;

mod auth;
mod create;
mod delete;
mod edit;
mod get;
mod set_enable;
mod toggle_enable;

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

    #[method(name = "toggle_enable")]
    async fn toggle_enable(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

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
        create::create(token, name, cron_expression, cron_type).await
    }

    async fn edit(
        &self,
        token: String,
        name: String,
        cron_expression: String,
        cron_type: CronType,
    ) -> RpcResult<Box<RawValue>> {
        edit::edit(token, name, cron_expression, cron_type).await
    }

    async fn get(&self, token: String) -> RpcResult<Box<RawValue>> {
        get::get(token).await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        delete::delete(token, name).await
    }

    async fn toggle_enable(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        toggle_enable::toggle_enable(token, name).await
    }

    async fn set_enable(
        &self,
        token: String,
        name: String,
        enable: bool,
    ) -> RpcResult<Box<RawValue>> {
        set_enable::set_enable(token, name, enable).await
    }
}
