use crate::rpc::{RpcHelper, token_identity};
use crate::rpc_exec;
use jsonrpsee::core::RpcResult;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use serde_json::value::RawValue;
use tracing::Instrument;

pub mod auth;
mod create;
mod delete;
mod exec_sql;
mod exec_templating;
mod list;
mod read;
mod update;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct NameParam {
    name: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RenameParam {
    name: String,
    new_name: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ExecSqlParam {
    name: String,
    sql: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[rpc(server, namespace = "db")]
pub trait Rpc {
    #[method(name = "create")]
    async fn create(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "read")]
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "update")]
    async fn update(
        &self,
        token: String,
        name: String,
        new_name: String,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "list")]
    async fn list(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "exec_sql")]
    async fn exec_sql(
        &self,
        token: String,
        name: String,
        sql: String,
        params: Option<serde_json::Value>,
    ) -> RpcResult<Box<RawValue>>;

    #[method(name = "exec_templating")]
    async fn exec_templating(
        &self,
        token: String,
        name: String,
        sql: String,
        params: Option<serde_json::Value>,
    ) -> RpcResult<Box<RawValue>>;
}

pub struct DbRpcImpl;

impl RpcHelper for DbRpcImpl {}

#[async_trait]
impl RpcServer for DbRpcImpl {
    async fn create(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::create", token_key = tk, username = un, name = %name);
        async { rpc_exec!(create::create(token, name).await) }
            .instrument(span)
            .await
    }

    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::read", token_key = tk, username = un, name = %name);
        async { rpc_exec!(read::read(token, name).await) }
            .instrument(span)
            .await
    }

    async fn update(
        &self,
        token: String,
        name: String,
        new_name: String,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::update", token_key = tk, username = un, name = %name, new_name = %new_name);
        async { rpc_exec!(update::update(token, name, new_name).await) }
            .instrument(span)
            .await
    }

    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    async fn list(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::list", token_key = tk, username = un);
        async { rpc_exec!(list::list(token).await) }
            .instrument(span)
            .await
    }

    async fn exec_sql(
        &self,
        token: String,
        name: String,
        sql: String,
        params: Option<serde_json::Value>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::exec_sql", token_key = tk, username = un, name = %name, sql_len = sql.len());
        async { rpc_exec!(exec_sql::exec_sql(token, name, sql, params).await) }
            .instrument(span)
            .await
    }

    async fn exec_templating(
        &self,
        token: String,
        name: String,
        sql: String,
        params: Option<serde_json::Value>,
    ) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::exec_templating", token_key = tk, username = un, name = %name, sql_len = sql.len());
        async { rpc_exec!(exec_templating::exec_templating(token, name, sql, params).await) }
            .instrument(span)
            .await
    }
}

/// Build and return the `db` RPC module, ready to merge into the server's RPC router.
#[must_use]
pub fn rpc_module() -> jsonrpsee::RpcModule<DbRpcImpl> {
    DbRpcImpl.into_rpc()
}
