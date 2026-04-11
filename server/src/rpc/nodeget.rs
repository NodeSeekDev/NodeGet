use crate::rpc::RpcHelper;
use crate::rpc::{rpc_exec, token_identity};
use crate::{RELOAD_NOTIFY, SERVER_CONFIG, SERVER_CONFIG_PATH};
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use nodeget_lib::config::server::ServerConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;

#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    #[method(name = "version")]
    async fn version(&self) -> Value;

    #[method(name = "uuid")]
    async fn uuid(&self) -> String;

    #[method(name = "list_all_agent_uuid")]
    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "read_config")]
    async fn read_config(&self, token: String) -> RpcResult<String>;

    #[method(name = "edit_config")]
    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool>;

    #[method(name = "database_storage")]
    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>>;

    #[method(name = "log")]
    async fn log(&self, token: String) -> RpcResult<Box<RawValue>>;
}

#[derive(Clone)]
pub struct NodegetServerRpcImpl;

impl RpcHelper for NodegetServerRpcImpl {}

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    async fn hello(&self) -> String {
        let span = tracing::info_span!(target: "rpc", "nodeget-server::hello");
        async {
            tracing::info!(target: "rpc", "request received");
            let response = "NodeGet Server Is Running!".to_string();
            tracing::info!(target: "rpc", response = %response, "request completed");
            response
        }.instrument(span).await
    }

    async fn version(&self) -> Value {
        let span = tracing::info_span!(target: "rpc", "nodeget-server::version");
        async {
            tracing::info!(target: "rpc", "request received");
            let response = serde_json::to_value(NodeGetVersion::get()).unwrap();
            tracing::info!(target: "rpc", response = %response, "request completed");
            response
        }.instrument(span).await
    }

    async fn uuid(&self) -> String {
        let span = tracing::info_span!(target: "rpc", "nodeget-server::uuid");
        async {
            tracing::info!(target: "rpc", "request received");
            let response = SERVER_CONFIG
                .get()
                .and_then(|cfg| cfg.read().ok().map(|c| c.server_uuid.to_string()))
                .unwrap_or_default();
            tracing::info!(target: "rpc", response = %response, "request completed");
            response
        }.instrument(span).await
    }

    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "rpc", "nodeget-server::list_all_agent_uuid", token_key = tk, username = un);
        async { rpc_exec!(list_all_agent_uuid::list_all_agent_uuid(token).await) }.instrument(span).await
    }

    async fn read_config(&self, token: String) -> RpcResult<String> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "rpc", "nodeget-server::read_config", token_key = tk, username = un);
        async {
            tracing::info!(target: "rpc", "request received");
            match config_ops::read_config(token).await {
                Ok(s) => {
                    tracing::info!(target: "rpc", response_len = s.len(), "request completed");
                    Ok(s)
                }
                Err(e) => {
                    tracing::error!(target: "rpc", error = %e, "request failed");
                    Err(e)
                }
            }
        }.instrument(span).await
    }

    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "rpc", "nodeget-server::edit_config", token_key = tk, username = un, config_len = config_string.len());
        async {
            tracing::info!(target: "rpc", "request received");
            match config_ops::edit_config(token, config_string).await {
                Ok(b) => {
                    tracing::info!(target: "rpc", response = b, "request completed");
                    Ok(b)
                }
                Err(e) => {
                    tracing::error!(target: "rpc", error = %e, "request failed");
                    Err(e)
                }
            }
        }.instrument(span).await
    }

    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "rpc", "nodeget-server::database_storage", token_key = tk, username = un);
        async { rpc_exec!(database_storage::database_storage(token).await) }.instrument(span).await
    }

    async fn log(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "rpc", "nodeget-server::log", token_key = tk, username = un);
        async { rpc_exec!(log_query::query_logs(token).await) }.instrument(span).await
    }
}

mod config_ops {
    use super::{
        NodegetError, RELOAD_NOTIFY, RpcResult, SERVER_CONFIG_PATH, ServerConfig, TokenOrAuth,
    };
    use crate::token::super_token::check_super_token;
    use std::path::Path;

    // 验证配置文件路径，防止路径遍历攻击
    fn validate_config_path(config_path: &str) -> anyhow::Result<&Path> {
        let path = Path::new(config_path);

        // 获取当前工作目录作为允许的基础目录
        let current_dir = std::env::current_dir()
            .map_err(|e| NodegetError::Other(format!("Cannot determine working directory: {e}")))?;

        // 获取规范化路径（解析符号链接和相对路径）
        let canonical_path = path
            .canonicalize()
            .map_err(|e| NodegetError::InvalidInput(format!("Invalid config path: {e}")))?;

        // 验证路径在允许目录内
        if !canonical_path.starts_with(&current_dir) {
            return Err(NodegetError::PermissionDenied(
                "Config path must be within working directory".to_owned(),
            )
            .into());
        }

        // 验证是文件而非目录
        if !canonical_path.is_file() {
            return Err(NodegetError::InvalidInput(
                "Config path must be a regular file".to_owned(),
            )
            .into());
        }

        Ok(path)
    }

    async fn ensure_super_token(token: &str) -> anyhow::Result<()> {
        let token_or_auth = TokenOrAuth::from_full_token(token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Super token required".to_owned(),
            )
            .into());
        }

        Ok(())
    }

    pub async fn read_config(token: String) -> RpcResult<String> {
        let process_logic = async {
            ensure_super_token(&token).await?;

            let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
                NodegetError::Other("Server config path not initialized".to_owned())
            })?;

            // 验证路径安全性，防止路径遍历
            validate_config_path(config_path)?;

            let file = tokio::fs::read_to_string(config_path)
                .await
                .map_err(|e| NodegetError::Other(format!("Failed to read config file: {e}")))?;

            Ok(file)
        };

        match process_logic.await {
            Ok(result) => Ok(result),
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                Err(jsonrpsee::types::ErrorObject::owned(
                    nodeget_err.error_code() as i32,
                    format!("{nodeget_err}"),
                    None::<()>,
                ))
            }
        }
    }

    pub async fn edit_config(token: String, config_string: String) -> RpcResult<bool> {
        let process_logic = async {
            ensure_super_token(&token).await?;

            let _parsed: ServerConfig = toml::from_str(&config_string)
                .map_err(|e| NodegetError::ParseError(format!("Config parse error: {e}")))?;

            let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
                NodegetError::Other("Server config path not initialized".to_owned())
            })?;

            // 验证路径安全性，防止路径遍历
            validate_config_path(config_path)?;

            // 使用临时文件+原子重命名，确保写入完整性
            let temp_path = format!("{}.tmp", config_path);
            tokio::fs::write(&temp_path, config_string)
                .await
                .map_err(|e| {
                    NodegetError::Other(format!("Failed to write temp config file: {e}"))
                })?;

            tokio::fs::rename(&temp_path, config_path)
                .await
                .map_err(|e| {
                    // 清理临时文件
                    let _ = tokio::fs::remove_file(&temp_path);
                    NodegetError::Other(format!("Failed to rename config file: {e}"))
                })?;

            if let Some(notify) = RELOAD_NOTIFY.get() {
                notify.notify_one();
            }

            Ok(true)
        };

        match process_logic.await {
            Ok(result) => Ok(result),
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                Err(jsonrpsee::types::ErrorObject::owned(
                    nodeget_err.error_code() as i32,
                    format!("{nodeget_err}"),
                    None::<()>,
                ))
            }
        }
    }
}

mod list_all_agent_uuid {
    use crate::rpc::{NodegetServerRpcImpl, RpcHelper};
    use crate::token::get::get_token;
    use crate::token::super_token::check_super_token;
    use jsonrpsee::core::RpcResult;
    use nodeget_lib::error::NodegetError;
    use nodeget_lib::permission::data_structure::{NodeGet, Permission, Scope};
    use nodeget_lib::permission::token_auth::TokenOrAuth;
    use nodeget_lib::utils::get_local_timestamp_ms_i64;
    use sea_orm::{FromQueryResult, Statement};
    use serde::Serialize;
    use serde_json::value::RawValue;
    use std::collections::HashSet;
    use uuid::Uuid;

    #[derive(FromQueryResult)]
    struct UuidRow {
        uuid: Uuid,
    }

    enum AgentUuidListPermission {
        All,
        Scoped(HashSet<Uuid>),
    }

    #[derive(Serialize)]
    struct ListAllAgentUuidResponse {
        uuids: Vec<Uuid>,
    }

    pub async fn list_all_agent_uuid(token: String) -> RpcResult<Box<RawValue>> {
        let process_logic = async {
            let permission = resolve_list_agent_uuid_permission(&token).await?;

            let db = NodegetServerRpcImpl::get_db()?;
            let all_uuids = fetch_all_agent_uuids(db).await?;
            let uuids = match permission {
                AgentUuidListPermission::All => all_uuids,
                AgentUuidListPermission::Scoped(allowed) => all_uuids
                    .into_iter()
                    .filter(|uuid| allowed.contains(uuid))
                    .collect(),
            };

            let response = ListAllAgentUuidResponse { uuids };
            let json_str = serde_json::to_string(&response)
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

            RawValue::from_string(json_str)
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
        };

        match process_logic.await {
            Ok(result) => Ok(result),
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                Err(jsonrpsee::types::ErrorObject::owned(
                    nodeget_err.error_code() as i32,
                    format!("{nodeget_err}"),
                    None::<()>,
                ))
            }
        }
    }

    async fn resolve_list_agent_uuid_permission(
        token: &str,
    ) -> anyhow::Result<AgentUuidListPermission> {
        let token_or_auth = TokenOrAuth::from_full_token(token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
        if is_super_token {
            return Ok(AgentUuidListPermission::All);
        }

        let token_info = get_token(&token_or_auth).await?;
        let now = get_local_timestamp_ms_i64()?;

        if let Some(from) = token_info.timestamp_from
            && now < from
        {
            return Err(NodegetError::PermissionDenied("Token is not yet valid".to_owned()).into());
        }

        if let Some(to) = token_info.timestamp_to
            && now > to
        {
            return Err(NodegetError::PermissionDenied("Token has expired".to_owned()).into());
        }

        let mut has_global_list_permission = false;
        let mut nodeget_scoped_uuids: HashSet<Uuid> = HashSet::new();
        let mut operable_scoped_uuids: HashSet<Uuid> = HashSet::new();

        for limit in &token_info.token_limit {
            let has_list_permission = limit
                .permissions
                .iter()
                .any(|perm| matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid)));

            if has_list_permission {
                if limit
                    .scopes
                    .iter()
                    .any(|scope| matches!(scope, Scope::Global))
                {
                    has_global_list_permission = true;
                }

                for scope in &limit.scopes {
                    if let Scope::AgentUuid(uuid) = scope {
                        nodeget_scoped_uuids.insert(*uuid);
                    }
                }
            }

            // "可操作" = 对该 AgentUuid Scope 至少拥有一种非 NodeGet::ListAllAgentUuid 的权限
            let has_any_operation_permission = limit
                .permissions
                .iter()
                .any(|perm| !matches!(perm, Permission::NodeGet(NodeGet::ListAllAgentUuid)));

            if has_any_operation_permission {
                for scope in &limit.scopes {
                    if let Scope::AgentUuid(uuid) = scope {
                        operable_scoped_uuids.insert(*uuid);
                    }
                }
            }
        }

        if has_global_list_permission {
            return Ok(AgentUuidListPermission::All);
        }

        if nodeget_scoped_uuids.is_empty() {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Insufficient NodeGet ListAllAgentUuid permissions".to_owned(),
            )
            .into());
        }

        let allowed_scoped_uuids: HashSet<Uuid> = nodeget_scoped_uuids
            .into_iter()
            .filter(|uuid| operable_scoped_uuids.contains(uuid))
            .collect();

        Ok(AgentUuidListPermission::Scoped(allowed_scoped_uuids))
    }

    async fn fetch_all_agent_uuids(db: &sea_orm::DatabaseConnection) -> anyhow::Result<Vec<Uuid>> {
        // 使用 UNION 合并三个表的查询，数据库层面去重，效率最高
        // UNION 自动去重，UNION ALL 不去重
        let sql = r"
            SELECT uuid FROM static_monitoring
            UNION
            SELECT uuid FROM dynamic_monitoring
            UNION
            SELECT uuid FROM task
            ORDER BY uuid
        ";

        let db_backend = db.get_database_backend();
        let statement = Statement::from_string(db_backend, sql.to_string());

        let rows = UuidRow::find_by_statement(statement)
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let uuids: Vec<Uuid> = rows.into_iter().map(|row| row.uuid).collect();

        Ok(uuids)
    }
}

mod database_storage {
    use crate::rpc::{NodegetServerRpcImpl, RpcHelper};
    use crate::token::super_token::check_super_token;
    use jsonrpsee::core::RpcResult;
    use nodeget_lib::error::NodegetError;
    use nodeget_lib::permission::token_auth::TokenOrAuth;
    use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
    use serde::Serialize;
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;

    /// 需要查询的表名列表（排除 seaql_migrations）
    const TABLE_NAMES: &[&str] = &[
        "static_monitoring",
        "dynamic_monitoring",
        "task",
        "token",
        "kv",
        "crontab",
        "crontab_result",
        "js_worker",
        "js_result",
    ];

    #[derive(FromQueryResult)]
    struct TableSizeRow {
        table_name: String,
        table_size: i64,
    }

    #[derive(Serialize)]
    struct DatabaseStorageResponse {
        /// 各表存储占用（字节）
        tables: BTreeMap<String, i64>,
        /// 数据库总大小（字节），所有表之和
        total: i64,
    }

    pub async fn database_storage(token: String) -> RpcResult<Box<RawValue>> {
        let process_logic = async {
            // 验证 super token 权限
            let token_or_auth = TokenOrAuth::from_full_token(&token)
                .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

            let is_super = check_super_token(&token_or_auth)
                .await
                .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

            if !is_super {
                return Err(NodegetError::PermissionDenied(
                    "Permission Denied: Super token required".to_owned(),
                )
                .into());
            }

            let db = NodegetServerRpcImpl::get_db()?;
            let tables = match db.get_database_backend() {
                DatabaseBackend::Postgres => query_postgres(db).await?,
                DatabaseBackend::Sqlite => query_sqlite(db).await?,
                backend => {
                    return Err(NodegetError::Other(format!(
                        "Unsupported database backend: {backend:?}"
                    ))
                    .into())
                }
            };

            let total: i64 = tables.values().sum();
            let response = DatabaseStorageResponse { tables, total };

            let json_str = serde_json::to_string(&response)
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

            RawValue::from_string(json_str)
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
        };

        match process_logic.await {
            Ok(result) => Ok(result),
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                Err(jsonrpsee::types::ErrorObject::owned(
                    nodeget_err.error_code() as i32,
                    format!("{nodeget_err}"),
                    None::<()>,
                ))
            }
        }
    }

    /// PostgreSQL: 使用 pg_total_relation_size() 查询各表总大小（含索引和 TOAST）
    async fn query_postgres(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
        // 使用 unnest 将表名数组展开，一次查询获取所有表的大小
        let sql = r#"
            SELECT
                t.name AS table_name,
                COALESCE(pg_total_relation_size(t.name::regclass), 0) AS table_size
            FROM unnest($1::text[]) AS t(name)
            ORDER BY t.name
        "#;

        let table_names: Vec<String> = TABLE_NAMES.iter().map(|s| s.to_string()).collect();

        let rows = TableSizeRow::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [table_names.into()],
        ))
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let mut result = BTreeMap::new();
        for row in rows {
            result.insert(row.table_name, row.table_size);
        }

        Ok(result)
    }

    /// SQLite: 使用 dbstat 虚拟表查询各表占用的页面总大小
    async fn query_sqlite(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
        let mut result = BTreeMap::new();

        for &table_name in TABLE_NAMES {
            // dbstat 虚拟表在 SQLite 编译时需启用 SQLITE_ENABLE_DBSTAT_VTAB
            // sqlx 的 bundled SQLite 默认启用此选项
            let sql = "SELECT COALESCE(SUM(pgsize), 0) AS table_size FROM dbstat WHERE name = ?";

            #[derive(FromQueryResult)]
            struct SizeRow {
                table_size: i64,
            }

            let row = SizeRow::find_by_statement(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                sql,
                [table_name.into()],
            ))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

            let size = row.map(|r| r.table_size).unwrap_or(0);
            result.insert(table_name.to_string(), size);
        }

        Ok(result)
    }
}

mod log_query {
    use crate::logging;
    use crate::token::super_token::check_super_token;
    use jsonrpsee::core::RpcResult;
    use nodeget_lib::error::NodegetError;
    use nodeget_lib::permission::token_auth::TokenOrAuth;
    use serde_json::value::RawValue;

    pub async fn query_logs(token: String) -> RpcResult<Box<RawValue>> {
        let process_logic = async {
            let token_or_auth = TokenOrAuth::from_full_token(&token)
                .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

            let is_super = check_super_token(&token_or_auth)
                .await
                .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

            if !is_super {
                return Err(NodegetError::PermissionDenied(
                    "Permission Denied: Super token required".to_owned(),
                )
                .into());
            }

            let logs = logging::get_memory_logs();

            let json_str = serde_json::to_string(&logs)
                .map_err(|e| NodegetError::SerializationError(e.to_string()))?;

            RawValue::from_string(json_str)
                .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
        };

        match process_logic.await {
            Ok(result) => Ok(result),
            Err(e) => {
                let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
                Err(jsonrpsee::types::ErrorObject::owned(
                    nodeget_err.error_code() as i32,
                    format!("{nodeget_err}"),
                    None::<()>,
                ))
            }
        }
    }
}
