use crate::rpc::RpcHelper;
use crate::rpc::{rpc_exec, token_identity};
use crate::{RELOAD_NOTIFY, SERVER_CONFIG, SERVER_CONFIG_PATH};
use jsonrpsee::core::{RpcResult, SubscriptionResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use nodeget_lib::config::server::ServerConfig;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::Value;
use serde_json::value::RawValue;
use tracing::Instrument;
use uuid::Uuid;

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

    #[subscription(name = "stream_log", item = Value, unsubscribe = "unsubscribe_stream_log")]
    async fn stream_log(&self, token: String, log_filter: String) -> SubscriptionResult;
}

#[derive(Clone)]
pub struct NodegetServerRpcImpl;

impl RpcHelper for NodegetServerRpcImpl {}

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    async fn hello(&self) -> String {
        let span = tracing::info_span!(target: "server", "nodeget-server::hello");
        async {
            let response = "NodeGet Server Is Running!".to_string();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    async fn version(&self) -> Value {
        let span = tracing::info_span!(target: "server", "nodeget-server::version");
        async {
            let response = serde_json::to_value(NodeGetVersion::get()).unwrap();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    async fn uuid(&self) -> String {
        let span = tracing::info_span!(target: "server", "nodeget-server::uuid");
        async {
            let response = SERVER_CONFIG
                .get()
                .and_then(|cfg| cfg.read().ok().map(|c| c.server_uuid.to_string()))
                .unwrap_or_default();
            tracing::debug!(target: "server", response = %response, "request completed");
            response
        }
        .instrument(span)
        .await
    }

    async fn list_all_agent_uuid(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::list_all_agent_uuid", token_key = tk, username = un);
        async { rpc_exec!(list_all_agent_uuid::list_all_agent_uuid(token).await) }
            .instrument(span)
            .await
    }

    async fn read_config(&self, token: String) -> RpcResult<String> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::read_config", token_key = tk, username = un);
        async {
            match config_ops::read_config(token).await {
                Ok(s) => {
                    tracing::debug!(target: "server", response_len = s.len(), "request completed");
                    Ok(s)
                }
                Err(e) => {
                    tracing::error!(target: "server", error = %e, "request failed");
                    Err(e)
                }
            }
        }
        .instrument(span)
        .await
    }

    async fn edit_config(&self, token: String, config_string: String) -> RpcResult<bool> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::edit_config", token_key = tk, username = un, config_len = config_string.len());
        async {
            match config_ops::edit_config(token, config_string).await {
                Ok(b) => {
                    tracing::debug!(target: "server", response = b, "request completed");
                    Ok(b)
                }
                Err(e) => {
                    tracing::error!(target: "server", error = %e, "request failed");
                    Err(e)
                }
            }
        }
        .instrument(span)
        .await
    }

    async fn database_storage(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::database_storage", token_key = tk, username = un);
        async { rpc_exec!(database_storage::database_storage(token).await) }
            .instrument(span)
            .await
    }

    async fn log(&self, token: String) -> RpcResult<Box<RawValue>> {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::log", token_key = tk, username = un);
        async { rpc_exec!(log_query::query_logs(token).await) }
            .instrument(span)
            .await
    }

    async fn stream_log(
        &self,
        subscription_sink: PendingSubscriptionSink,
        token: String,
        log_filter: String,
    ) -> SubscriptionResult {
        let (tk, un) = token_identity(&token);
        let span = tracing::info_span!(target: "server", "nodeget-server::stream_log", token_key = tk, username = un);
        let _guard = span.enter();

        // ── Authentication ──────────────────────────────────────────
        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(target: "server", error = %e, "token parse error, rejecting stream_log subscription");
                subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        101,
                        format!("Token Parse Error: {e}"),
                        None::<()>,
                    ))
                    .await;
                return Ok(());
            }
        };

        let is_super = match crate::token::super_token::check_super_token(&token_or_auth).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(target: "server", error = %e, "super token check failed, rejecting stream_log subscription");
                subscription_sink
                    .reject(jsonrpsee::types::ErrorObject::owned(
                        102,
                        format!("Permission check failed: {e}"),
                        None::<()>,
                    ))
                    .await;
                return Ok(());
            }
        };

        if !is_super {
            tracing::warn!(target: "server", "permission denied, rejecting stream_log subscription");
            subscription_sink
                .reject(jsonrpsee::types::ErrorObject::borrowed(
                    102,
                    "Permission Denied: Super token required",
                    None,
                ))
                .await;
            return Ok(());
        }

        // ── Accept subscription ─────────────────────────────────────
        let sink = subscription_sink.accept().await?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(512);
        let sub_id = Uuid::new_v4();

        let manager = crate::logging::get_stream_log_manager();
        // NOTE: no tracing calls here – add_subscriber holds the write lock
        manager.add_subscriber(sub_id, tx, &log_filter);

        // Log *after* the lock is released
        tracing::info!(target: "server", sub_id = %sub_id, filter = %log_filter, "stream_log subscription accepted");

        // Drop span guard before spawning the forwarding task
        drop(_guard);
        let forward_span = span.clone();
        let manager = manager.clone();

        tokio::spawn(async move {
            while let Some(entry) = rx.recv().await {
                let json_str = match serde_json::to_string(&entry) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let Ok(raw) = RawValue::from_string(json_str) else {
                    continue;
                };
                let msg = SubscriptionMessage::from(raw);
                if sink.send(msg).await.is_err() {
                    break;
                }
            }

            // NOTE: no tracing calls inside remove_subscriber (holds write lock)
            manager.remove_subscriber(&sub_id);
            // Log after lock is released
            tracing::info!(target: "server", sub_id = %sub_id, "stream_log subscriber disconnected, removed");
        }.instrument(forward_span));

        Ok(())
    }
}

mod config_ops {
    use super::{
        NodegetError, RELOAD_NOTIFY, RpcResult, SERVER_CONFIG_PATH, ServerConfig, TokenOrAuth,
    };
    use crate::token::super_token::check_super_token;
    use std::path::Path;
    use tracing::{debug, trace};

    // 验证配置文件路径，防止路径遍历攻击
    fn validate_config_path(config_path: &str) -> anyhow::Result<&Path> {
        trace!(target: "server", path = %config_path, "validating config path");
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
        trace!(target: "server", "checking super token");
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
        debug!(target: "server", "reading server config");
        let process_logic = async {
            ensure_super_token(&token).await?;
            debug!(target: "server", "Super token verified for read_config");

            let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
                NodegetError::Other("Server config path not initialized".to_owned())
            })?;

            // 验证路径安全性，防止路径遍历
            validate_config_path(config_path)?;
            debug!(target: "server", path = %config_path, "Config path validated for read");

            let file = tokio::fs::read_to_string(config_path)
                .await
                .map_err(|e| NodegetError::Other(format!("Failed to read config file: {e}")))?;
            debug!(target: "server", bytes = file.len(), "Config file read successfully");

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
        debug!(target: "server", config_len = config_string.len(), "editing server config");
        let process_logic = async {
            ensure_super_token(&token).await?;
            debug!(target: "server", "Super token verified for edit_config");

            let _parsed: ServerConfig = toml::from_str(&config_string)
                .map_err(|e| NodegetError::ParseError(format!("Config parse error: {e}")))?;
            debug!(target: "server", "Config string parsed successfully");

            let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
                NodegetError::Other("Server config path not initialized".to_owned())
            })?;

            // 验证路径安全性，防止路径遍历
            validate_config_path(config_path)?;
            debug!(target: "server", path = %config_path, "Config path validated");

            // 使用临时文件+原子重命名，确保写入完整性
            let temp_path = format!("{config_path}.tmp");
            tokio::fs::write(&temp_path, config_string)
                .await
                .map_err(|e| {
                    NodegetError::Other(format!("Failed to write temp config file: {e}"))
                })?;
            debug!(target: "server", temp_path = %temp_path, "Temp config file written");

            tokio::fs::rename(&temp_path, config_path)
                .await
                .map_err(|e| {
                    // 清理临时文件
                    let _ = tokio::fs::remove_file(&temp_path);
                    NodegetError::Other(format!("Failed to rename config file: {e}"))
                })?;
            debug!(target: "server", "Config file renamed from temp to target");

            if let Some(notify) = RELOAD_NOTIFY.get() {
                notify.notify_one();
                debug!(target: "server", "Config reload notification sent");
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
    use tracing::{debug, trace};
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
        debug!(target: "server", "listing all agent uuids");
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
            debug!(target: "server", uuid_count = response.uuids.len(), "list_all_agent_uuid completed");
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
        trace!(target: "server", "resolving agent uuid list permission");
        let token_or_auth = TokenOrAuth::from_full_token(token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;
        if is_super_token {
            trace!(target: "server", "Super token detected, granting All agent UUID access");
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
            trace!(target: "server", "Global ListAllAgentUuid permission found, granting All access");
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

        trace!(target: "server", allowed_count = allowed_scoped_uuids.len(), "Scoped agent UUID permission resolved");
        Ok(AgentUuidListPermission::Scoped(allowed_scoped_uuids))
    }

    async fn fetch_all_agent_uuids(db: &sea_orm::DatabaseConnection) -> anyhow::Result<Vec<Uuid>> {
        debug!(target: "server", "fetching all agent uuids");
        // Get UUIDs from monitoring_uuid cache (covers static/dynamic/summary)
        let uuid_cache = crate::monitoring_uuid_cache::MonitoringUuidCache::global();
        let mut uuid_set: std::collections::BTreeSet<Uuid> = uuid_cache.get_all_uuids().await.into_iter().collect();

        // Also include UUIDs from task table (not covered by monitoring_uuid)
        let sql = r"SELECT uuid FROM task";
        let db_backend = db.get_database_backend();
        let statement = Statement::from_string(db_backend, sql.to_string());

        let rows = UuidRow::find_by_statement(statement)
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        for row in rows {
            uuid_set.insert(row.uuid);
        }

        let uuids: Vec<Uuid> = uuid_set.into_iter().collect();
        debug!(target: "server", uuid_count = uuids.len(), "Fetched all agent UUIDs");

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
    use tracing::debug;

    /// 需要查询的表名列表（排除 `seaql_migrations`）
    const TABLE_NAMES: &[&str] = &[
        "monitoring_uuid",
        "static_monitoring",
        "dynamic_monitoring",
        "dynamic_monitoring_summary",
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
        debug!(target: "server", "querying database storage");
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
            debug!(target: "server", "Super token verified for database_storage");

            let db = NodegetServerRpcImpl::get_db()?;
            let tables = match db.get_database_backend() {
                DatabaseBackend::Postgres => query_postgres(db).await?,
                DatabaseBackend::Sqlite => query_sqlite(db).await?,
                backend => {
                    return Err(NodegetError::Other(format!(
                        "Unsupported database backend: {backend:?}"
                    ))
                    .into());
                }
            };

            let total: i64 = tables.values().sum();
            let response = DatabaseStorageResponse { tables, total };
            debug!(target: "server", total_bytes = total, "Database storage query completed");

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

    /// `PostgreSQL`: 使用 `pg_total_relation_size()` 查询各表总大小（含索引和 TOAST）
    async fn query_postgres(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
        debug!(target: "server", "querying postgres table sizes");
        // 使用 unnest 将表名数组展开，一次查询获取所有表的大小
        let sql = r"
            SELECT
                t.name AS table_name,
                COALESCE(pg_total_relation_size(t.name::regclass), 0) AS table_size
            FROM unnest($1::text[]) AS t(name)
            ORDER BY t.name
        ";

        let table_names: Vec<String> = TABLE_NAMES
            .iter()
            .map(ToString::to_string)
            .collect();

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
        debug!(target: "server", table_count = result.len(), "Postgres table sizes queried");

        Ok(result)
    }

    /// `SQLite`: 使用 dbstat 虚拟表查询各表占用的页面总大小
    async fn query_sqlite(db: &DatabaseConnection) -> anyhow::Result<BTreeMap<String, i64>> {
        debug!(target: "server", "querying sqlite table sizes");
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

            let size = row.map_or(0, |r| r.table_size);
            result.insert(table_name.to_string(), size);
        }
        debug!(target: "server", table_count = result.len(), "SQLite table sizes queried");

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
    use tracing::debug;

    pub async fn query_logs(token: String) -> RpcResult<Box<RawValue>> {
        debug!(target: "server", "querying in-memory logs");
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
            debug!(target: "server", "Super token verified for log query");

            let logs = logging::get_memory_logs();
            debug!(target: "server", log_count = logs.len(), "In-memory logs fetched");

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
