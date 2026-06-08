//! `db` RPC 命名空间 — 用户数据库 CRUD 与 SQL 执行
//!
//! 方法：`create` / `read` / `update` / `delete` / `list` / `exec_sql`
//! 所有方法通过 `rpc_exec!` 宏统一日志输出，通过 `token_identity` 追踪请求来源。

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
mod list;
mod read;
mod update;

/// 按名称查询参数（已定义但暂未使用，保留供反序列化扩展）
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct NameParam {
    /// 数据库名称
    name: String,
}

/// 重命名参数（已定义但暂未使用，保留供反序列化扩展）
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RenameParam {
    /// 原数据库名称
    name: String,
    /// 新数据库名称
    new_name: String,
}

/// SQL 执行参数（已定义但暂未使用，保留供反序列化扩展）
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ExecSqlParam {
    /// 数据库名称
    name: String,
    /// SQL 语句
    sql: String,
    #[serde(default)]
    /// SQL 参数（JSON 格式）
    params: Option<serde_json::Value>,
}

/// `db` RPC trait 定义，jsonrpsee 宏自动生成 `RpcServer` trait
#[rpc(server, namespace = "db")]
pub trait Rpc {
    /// 创建用户数据库，需 `SuperToken` 权限
    #[method(name = "create")]
    async fn create(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 查询用户数据库信息
    #[method(name = "read")]
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 重命名用户数据库，需 `SuperToken` 权限
    #[method(name = "update")]
    async fn update(
        &self,
        token: String,
        name: String,
        new_name: String,
    ) -> RpcResult<Box<RawValue>>;

    /// 删除用户数据库，需 `SuperToken` 权限
    #[method(name = "delete")]
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>>;

    /// 列出所有用户数据库，需 `SuperToken` 权限
    #[method(name = "list")]
    async fn list(&self, token: String) -> RpcResult<Box<RawValue>>;

    /// 在指定用户数据库上执行 SQL，需 Token 对应的数据库权限
    #[method(name = "exec_sql")]
    async fn exec_sql(
        &self,
        token: String,
        name: String,
        sql: String,
        params: Option<serde_json::Value>,
    ) -> RpcResult<Box<RawValue>>;
}

/// `db` RPC 实现，委托给各子模块函数
pub struct DbRpcImpl;

impl RpcHelper for DbRpcImpl {}

#[async_trait]
impl RpcServer for DbRpcImpl {
    /// 创建用户数据库，附带 tracing span 日志
    async fn create(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::create", token_key = tk, username = un, name = %name);
        async { rpc_exec!(create::create(token, name).await) }
            .instrument(span)
            .await
    }

    /// 查询用户数据库信息
    async fn read(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::read", token_key = tk, username = un, name = %name);
        async { rpc_exec!(read::read(token, name).await) }
            .instrument(span)
            .await
    }

    /// 重命名用户数据库
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

    /// 删除用户数据库
    async fn delete(&self, token: String, name: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::delete", token_key = tk, username = un, name = %name);
        async { rpc_exec!(delete::delete(token, name).await) }
            .instrument(span)
            .await
    }

    /// 列出所有用户数据库
    async fn list(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "db", "db::list", token_key = tk, username = un);
        async { rpc_exec!(list::list(token).await) }
            .instrument(span)
            .await
    }

    /// 在指定用户数据库上执行 SQL
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
}

/// 构建 `db` RPC 模块，供服务端合并到 RPC Router
#[must_use]
pub fn rpc_module() -> jsonrpsee::RpcModule<DbRpcImpl> {
    DbRpcImpl.into_rpc()
}
