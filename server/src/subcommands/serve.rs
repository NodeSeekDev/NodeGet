//! `serve` 子命令——HTTP/WebSocket 服务器启动与运行
//!
//! 职责：
//! 1. 初始化所有全局缓存和 trait 注入（依赖倒置）
//! 2. 构建路由表：JSON-RPC、静态文件、WebDAV、JS Worker HTTP 路由、Terminal WebSocket
//! 3. 启动 TCP 监听（可选 TLS）和 Unix Socket 监听
//! 4. 监听配置热重载信号，优雅关闭后由 main 循环重启
//!
//! 同时包含所有 trait 注入的具体实现（ServerPermissionChecker、MonitoringUuidProvider 等），
//! 这些结构体将 ng-* crate 的抽象接口桥接到 `ng_token` 的具体逻辑。
//! 统一权限校验器 `PermissionChecker`（ng-core）替代了原先散布在各 crate 的重复 trait 定义。

use axum::routing::any;
use axum::{extract::Path, http::StatusCode};
use base64::Engine as _;
use ng_config::config::server::ServerConfig;
use ng_config::get_reload_notify;
use ng_core::permission::data_structure::{Permission, Scope};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::js_worker;
use ng_js_runtime::RunType;
use ng_js_runtime::RuntimeLimits;
use ng_js_runtime::runtime_pool;
use ng_js_worker::ensure_bytecode_version;
use ng_static::cache::StaticCache;
use ng_static::ops::{get_static_path, resolve_safe_file_path};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::Service;
use tracing::{debug, error, info, warn};

use crate::rpc_nodeget::get_modules;
use crate::rpc_timing::RpcTimingMiddleware;

/// 启动服务器主循环
///
/// 内部步骤：
/// 1. 安装 rustls 默认 provider（TLS 所需）
/// 2. 初始化 Super Token
/// 3. 初始化所有全局缓存（Token、Monitoring、Static、Crontab、JS Runtime Pool、DB Registry）
/// 4. 注入所有 trait providers（PermissionChecker、MonitoringUuidProvider 等）
/// 5. 构建 RPC 模块和 axum 路由表
/// 6. 启动 TCP（可选 TLS）+ Unix Socket 监听
/// 7. 通过 `tokio::select!` 同时等待：服务器正常退出 或 热重载信号
/// 8. 退出前刷新 monitoring buffer、关闭 DB registry、清理 Unix socket 文件
#[allow(clippy::too_many_lines)]
pub async fn run(config: &ServerConfig) {
    let _ = rustls::crypto::ring::default_provider().install_default();

    super::init_or_skip_super_token().await;
    debug!(target: "server", "Super token initialization completed");

    // ── 初始化各缓存 ──────────────────────────────────────────────
    ng_token::TokenCache::init()
        .await
        .expect("Failed to initialize token cache");
    debug!(target: "server", "Token cache initialized");

    ng_monitoring::monitoring_uuid_cache::MonitoringUuidCache::init()
        .await
        .expect("Failed to initialize monitoring UUID cache");
    debug!(target: "server", "Monitoring UUID cache initialized");

    ng_monitoring::static_hash_cache::StaticHashCache::init();
    debug!(target: "server", "Static hash cache initialized");

    ng_monitoring::monitoring_last_cache::MonitoringLastCache::init();
    debug!(target: "server", "Monitoring last cache initialized");

    ng_static::StaticCache::init()
        .await
        .expect("Failed to initialize static cache");
    debug!(target: "server", "Static cache initialized");

    ng_crontab::CrontabCache::init()
        .await
        .expect("Failed to initialize crontab cache");
    debug!(target: "server", "Crontab cache initialized");

    runtime_pool::init_global_pool();
    debug!(target: "server", "JS runtime pool initialized");

    ng_monitoring::monitoring_buffer::init(config.monitoring_buffer.as_ref());
    debug!(target: "server", "Monitoring buffer initialized");

    let db_path = config.db_path.clone().unwrap_or_else(|| "./db/".to_owned());
    ng_db::DbRegistryManager::init(db_path).await;
    debug!(target: "server", "DB registry manager initialized");

    // ── 注入 trait providers ──────────────────────────────────────
    // 统一权限校验器：注册 PermissionChecker（供 ng-db、ng-kv、ng-static、ng-js-worker、ng-terminal、ng-task、ng-config 共用）
    ng_core::permission::permission_checker::set_permission_checker(std::sync::Arc::new(
        ServerPermissionChecker,
    ));
    debug!(target: "server", "unified PermissionChecker registered");

    // ng-task：注册 monitoring UUID provider
    ng_task::set_monitoring_uuid_provider(std::sync::Arc::new(TaskMonitoringUuidProvider));
    debug!(target: "server", "ng-task monitoring UUID provider registered");

    // ng-js-runtime：注册 JsWorkerService (inline_call + nodeget RPC dispatch)
    ng_js_runtime::js_worker_service::set_js_worker_service(Box::new(JsWorkerServiceImpl));
    debug!(target: "server", "ng-js-runtime JsWorkerService registered");

    // ng-crontab：注册 JsWorkerScheduler (cron 触发 JS Worker 任务)
    ng_crontab::task::set_js_worker_scheduler(std::sync::Arc::new(CronJsWorkerScheduler));
    debug!(target: "server", "ng-crontab JsWorkerScheduler registered");

    let rpc_module = get_modules();

    let (stop_handle, _server_handle) = jsonrpsee::server::stop_channel();
    let rpc_middleware =
        jsonrpsee::server::middleware::rpc::RpcServiceBuilder::new().layer_fn(move |service| {
            RpcTimingMiddleware {
                service,
                level: tracing::Level::TRACE,
            }
        });

    let jsonrpc_service = jsonrpsee::server::Server::builder()
        .set_rpc_middleware(rpc_middleware)
        .set_config(
            jsonrpsee::server::ServerConfig::builder()
                .max_connections(config.jsonrpc_max_connections.unwrap_or(100))
                .max_response_body_size(config.max_response_body_size.unwrap_or(100 * 1024 * 1024))
                .max_request_body_size(config.max_request_body_size.unwrap_or(10 * 1024 * 1024))
                .build(),
        )
        .to_service_builder()
        .build(rpc_module, stop_handle.clone());
    let jsonrpc_service_for_root = jsonrpc_service.clone();
    let landing_html = render_root_html(&config.server_uuid.to_string(), env!("CARGO_PKG_VERSION"));

    let jsonrpc_service_for_rpc = jsonrpc_service_for_root.clone();
    let landing_html_for_rpc = landing_html.clone();

    // 使用 ng-static::router::router() 提供静态文件和 WebDAV 路由
    let static_router = ng_static::router::router();

    let app =
        axum::Router::new()
            .route(
                "/",
                any(move |req: axum::extract::Request| {
                    let mut rpc_service = jsonrpc_service_for_root.clone();
                    let landing_html = landing_html.clone();
                    async move {
                        if is_websocket_upgrade(req.headers()) {
                            return rpc_service.call(req).await.unwrap();
                        }

                        if req.method() == axum::http::Method::GET {
                            let Some(cache) = StaticCache::global() else {
                                return axum::response::Response::builder()
                                    .status(StatusCode::OK)
                                    .header(
                                        axum::http::header::CONTENT_TYPE,
                                        "text/html; charset=utf-8",
                                    )
                                    .body(jsonrpsee::server::HttpBody::from(landing_html))
                                    .expect("Failed to build HTML response");
                            };
                            if let Some(model) = cache.get_http_root()
                                && model.enable != Some(false)
                            {
                                let path = req.uri().path().to_owned();
                                let method = req.method().clone();
                                return serve_static_file(&model.path, &path, model.cors, &method)
                                    .await;
                            }
                            return axum::response::Response::builder()
                                .status(StatusCode::OK)
                                .header(
                                    axum::http::header::CONTENT_TYPE,
                                    "text/html; charset=utf-8",
                                )
                                .body(jsonrpsee::server::HttpBody::from(landing_html))
                                .expect("Failed to build HTML response");
                        }

                        rpc_service.call(req).await.unwrap_or_else(|e| {
                            tracing::error!(target: "server", error = %e, "RPC call failed");
                            axum::http::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(jsonrpsee::server::HttpBody::from("Internal Server Error"))
                                .expect("Failed to build error response")
                        })
                    }
                }),
            )
            .route(
                "/nodeget/rpc",
                any(move |req: axum::extract::Request| {
                    let mut rpc_service = jsonrpc_service_for_rpc.clone();
                    let landing_html = landing_html_for_rpc.clone();
                    async move {
                        if is_websocket_upgrade(req.headers()) {
                            return rpc_service.call(req).await.unwrap();
                        }

                        if req.method() == axum::http::Method::GET {
                            let Some(cache) = StaticCache::global() else {
                                return axum::response::Response::builder()
                                    .status(StatusCode::OK)
                                    .header(
                                        axum::http::header::CONTENT_TYPE,
                                        "text/html; charset=utf-8",
                                    )
                                    .body(jsonrpsee::server::HttpBody::from(landing_html))
                                    .expect("Failed to build HTML response");
                            };
                            if let Some(model) = cache.get_http_root()
                                && model.enable != Some(false)
                            {
                                let path = req.uri().path().to_owned();
                                let method = req.method().clone();
                                return serve_static_file(&model.path, &path, model.cors, &method)
                                    .await;
                            }
                            return axum::response::Response::builder()
                                .status(StatusCode::OK)
                                .header(
                                    axum::http::header::CONTENT_TYPE,
                                    "text/html; charset=utf-8",
                                )
                                .body(jsonrpsee::server::HttpBody::from(landing_html))
                                .expect("Failed to build HTML response");
                        }

                        rpc_service.call(req).await.unwrap_or_else(|e| {
                            tracing::error!(target: "server", error = %e, "RPC call failed");
                            axum::http::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(jsonrpsee::server::HttpBody::from("Internal Server Error"))
                                .expect("Failed to build error response")
                        })
                    }
                }),
            )
            // 合并 ng-static 的静态文件和 WebDAV 路由
            .merge(static_router)
            .route(
                "/worker-route/{route_name}",
                any(
                    |Path(route_name): Path<String>, req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            .route(
                "/worker-route/{route_name}/",
                any(
                    |Path(route_name): Path<String>, req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            .route(
                "/worker-route/{route_name}/{*path}",
                any(
                    |Path((route_name, _path)): Path<(String, String)>,
                     req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            // 新的统一前缀 /nodeget/worker-route/*，与 /nodeget/static/* 保持一致。
            // 旧的 /worker-route/* 保留用于过渡，后续版本将移除。
            .route(
                "/nodeget/worker-route/{route_name}",
                any(
                    |Path(route_name): Path<String>, req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            .route(
                "/nodeget/worker-route/{route_name}/",
                any(
                    |Path(route_name): Path<String>, req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            .route(
                "/nodeget/worker-route/{route_name}/{*path}",
                any(
                    |Path((route_name, _path)): Path<(String, String)>,
                     req: axum::extract::Request| async move {
                        handle_js_worker_route(route_name, req).await
                    },
                ),
            )
            // Terminal WebSocket 路由（独立 state）
            .merge(ng_terminal::router())
            .fallback(any(move |req: axum::extract::Request| {
                let mut rpc_service = jsonrpc_service.clone();
                async move {
                    if is_websocket_upgrade(req.headers()) {
                        return rpc_service.call(req).await.unwrap_or_else(|e| {
                            tracing::error!(target: "server", error = %e, "RPC call failed");
                            axum::http::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(jsonrpsee::server::HttpBody::from("Internal Server Error"))
                                .expect("Failed to build error response")
                        });
                    }
                    if let Some(cache) = StaticCache::global()
                        && let Some(model) = cache.get_http_root()
                    {
                        let path = req.uri().path().to_owned();
                        let method = req.method().clone();
                        return serve_static_file(&model.path, &path, model.cors, &method).await;
                    }
                    rpc_service.call(req).await.unwrap_or_else(|e| {
                            tracing::error!(target: "server", error = %e, "RPC call failed");
                            axum::http::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(jsonrpsee::server::HttpBody::from("Internal Server Error"))
                                .expect("Failed to build error response")
                        })
                }
            }));

    ng_crontab::init_crontab_worker();
    debug!(target: "server", "Crontab worker initialized");

    #[cfg(not(target_os = "windows"))]
    let mut unix_server_task: Option<tokio::task::JoinHandle<()>> = None;
    #[cfg(not(target_os = "windows"))]
    let mut unix_socket_path: Option<String> = None;

    #[cfg(not(target_os = "windows"))]
    if config.enable_unix_socket.unwrap_or(false) {
        let socket_path = config
            .unix_socket_path
            .clone()
            .unwrap_or_else(|| "/var/lib/nodeget.sock".to_owned());

        match bind_unix_listener(socket_path.as_str()).await {
            Ok(unix_listener) => {
                let unix_app = app.clone();
                unix_socket_path = Some(socket_path.clone());
                unix_server_task = Some(tokio::spawn(async move {
                    if let Err(e) = axum::serve(unix_listener, unix_app.into_make_service()).await {
                        tracing::error!(target: "server", error = %e, "Unix socket server stopped with error");
                    }
                }));
                info!(target: "server", socket_path = %socket_path, "Unix socket listener started");
            }
            Err(e) => {
                tracing::error!(target: "server", error = %e, "Failed to bind unix socket listener");
            }
        }
    }

    let addr: SocketAddr = config.ws_listener.parse().unwrap_or_else(|e| {
        error!(target: "server", address = %config.ws_listener, error = %e, "failed to parse listen address");
        panic!("Invalid listen address '{}': {e}", config.ws_listener);
    });

    let tls_enabled = config.tls_cert.is_some() && config.tls_key.is_some();
    if tls_enabled {
        let cert_path = config.tls_cert.as_deref().unwrap();
        let key_path = config.tls_key.as_deref().unwrap();
        info!(target: "server", address = %addr, cert = %cert_path, key = %key_path, "Server listening on TCP with TLS");
        let tls_config = build_http1_only_tls_config(cert_path, key_path)
            .await
            .unwrap_or_else(|e| panic!("Failed to load TLS config: {e}"));
        let serve_future =
            axum_server::bind_rustls(addr, tls_config).serve(app.into_make_service());
        tokio::pin!(serve_future);

        tokio::select! {
            biased;
            result = &mut serve_future => {
                result.unwrap();
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
                ng_db::DbRegistryManager::global()
                    .expect("DbRegistryManager not initialized at shutdown")
                    .shutdown()
                    .await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), stop_handle.shutdown()).await;
                #[cfg(not(target_os = "windows"))]
                if let Some(task) = unix_server_task.take() {
                    task.abort();
                }
                #[cfg(not(target_os = "windows"))]
                cleanup_unix_socket_file(unix_socket_path.as_deref()).await;
            }
            () = get_reload_notify()
                .expect("Reload notify not initialized")
                .notified() => {
                info!(target: "server", "Config reload requested, stopping TLS server...");
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
                ng_db::DbRegistryManager::global()
                    .expect("DbRegistryManager not initialized at shutdown")
                    .shutdown()
                    .await;
                let stop_handle = stop_handle.clone();
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), stop_handle.shutdown()).await;
                });
                #[cfg(not(target_os = "windows"))]
                if let Some(task) = unix_server_task.take() {
                    task.abort();
                }
                #[cfg(not(target_os = "windows"))]
                cleanup_unix_socket_file(unix_socket_path.as_deref()).await;
            }
        }
    } else {
        info!(target: "server", address = %addr, "Server listening on TCP");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| {
                error!(target: "server", address = %addr, error = %e, "failed to bind TCP listener");
                panic!("Failed to bind to {addr}: {e}");
            });
        let serve_future = IntoFuture::into_future(axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        ));
        tokio::pin!(serve_future);

        tokio::select! {
            biased;
            result = &mut serve_future => {
                result.unwrap();
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
                ng_db::DbRegistryManager::global()
                    .expect("DbRegistryManager not initialized at shutdown")
                    .shutdown()
                    .await;
                #[cfg(not(target_os = "windows"))]
                if let Some(task) = unix_server_task.take() {
                    task.abort();
                }
                #[cfg(not(target_os = "windows"))]
                cleanup_unix_socket_file(unix_socket_path.as_deref()).await;
            }
            () = get_reload_notify()
                .expect("Reload notify not initialized")
                .notified() => {
                info!(target: "server", "Config reload requested, stopping server for restart...");
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
                ng_db::DbRegistryManager::global()
                    .expect("DbRegistryManager not initialized at shutdown")
                    .shutdown()
                    .await;
                let stop_handle = stop_handle.clone();
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), stop_handle.shutdown()).await;
                });
                #[cfg(not(target_os = "windows"))]
                if let Some(task) = unix_server_task.take() {
                    task.abort();
                }
                #[cfg(not(target_os = "windows"))]
                cleanup_unix_socket_file(unix_socket_path.as_deref()).await;
            }
        }
    }
}

/// 构建 ALPN 仅广播 `http/1.1` 的 TLS 配置
///
/// `axum-server` 默认 ALPN 为 `[h2, http/1.1]`，客户端会优先协商 HTTP/2，
/// 导致 h2 frame 生命周期开销（samply 显示 14-33% tokio worker 时间）。
/// 本服务器所有入站连接均为 WebSocket（HTTP/1.1 upgrade）或 JSON-RPC（HTTP/1.1），
/// 不需要 HTTP/2，限制 ALPN 消除 h2 开销。
async fn build_http1_only_tls_config(
    cert_path: &str,
    key_path: &str,
) -> std::io::Result<axum_server::tls_rustls::RustlsConfig> {
    let cert_pem = tokio::fs::read(cert_path).await?;
    let key_pem = tokio::fs::read(key_path).await?;

    let certs: Vec<_> = CertificateDer::pem_slice_iter(&cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let key = PrivateKeyDer::from_pem_slice(&key_pem)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // 仅广播 http/1.1，阻止客户端协商 HTTP/2
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(axum_server::tls_rustls::RustlsConfig::from_config(
        Arc::new(server_config),
    ))
}

/// 渲染根路径着陆页 HTML
///
/// 包含服务器 UUID、版本号和常用链接。
fn render_root_html(serv_uuid: &str, serv_version: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NodeGet Server Backend</title>
    <meta name="description" content="Next-generation server monitoring and management tools">
    <link rel="icon" href="https://nodeget.com/logo.png">
</head>
<body>
    <h1>Welcome to NodeGet</h1>
    <p>Next-generation server monitoring and management tools</p>
    <h2>Server</h2>
    <p>UUID: <span>{serv_uuid}</span></p>
    <p>Version: <span>{serv_version}</span></p>
    <h2>Useful Links</h2>
    <ul>
        <li><a href="https://dash.nodeget.com">Dashboard</a></li>
        <li><a href="https://nodeget.com">Official Website</a></li>
        <li><a href="https://github.com/nodeseekdev/nodeget">Github Project</a></li>
    </ul>
</body>
</html>"#
    )
}

/// 判断 HTTP 请求是否为 WebSocket 升级请求
///
/// 检查 `Upgrade: websocket` 和 `Connection: upgrade` 两个头字段。
fn is_websocket_upgrade(headers: &axum::http::HeaderMap) -> bool {
    let has_upgrade_header = headers
        .get(axum::http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));

    let has_connection_upgrade = headers
        .get(axum::http::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|segment| segment.trim().eq_ignore_ascii_case("upgrade"))
        });

    has_upgrade_header && has_connection_upgrade
}

/// JS Worker HTTP 路由的请求头结构
#[derive(Debug, Serialize)]
struct JsRouteHeader {
    /// 头字段名
    name: String,
    /// 头字段值
    value: String,
}

/// JS Worker HTTP 路由的输入参数
#[derive(Debug, Serialize)]
struct JsRouteInput {
    /// HTTP 方法（GET/POST 等）
    method: String,
    /// 完整请求 URL
    url: String,
    /// 请求头列表
    headers: Vec<JsRouteHeader>,
    /// 请求体（Base64 编码）
    body_base64: String,
}

/// JS Worker HTTP 路由的输出结构
#[derive(Debug, Deserialize)]
struct JsRouteOutput {
    /// HTTP 状态码
    status: u16,
    /// 响应头列表
    headers: Vec<JsRouteOutputHeader>,
    /// 响应体（Base64 编码）
    body_base64: String,
}

/// JS Worker HTTP 路由输出的响应头
#[derive(Debug, Deserialize)]
struct JsRouteOutputHeader {
    /// 头字段名
    name: String,
    /// 头字段值
    value: String,
}

/// 处理 JS Worker HTTP 路由请求
///
/// 内部步骤：
/// 1. 校验 `route_name` 非空
/// 2. 提取客户端 IP（从 `ConnectInfo` 或默认 127.0.0.1）
/// 3. 构建完整 URL（处理 x-forwarded-proto 和 Host 头）
/// 4. 注入 ng-connecting-ip 头供 JS 脚本获取真实 IP
/// 5. 读取请求体（上限 8MB）并 Base64 编码
/// 6. 从数据库查询 `route_name` 对应的 JS Worker
/// 7. 在 `QuickJS` 运行时池中执行脚本
/// 8. 解析脚本输出（`JsRouteOutput`），构建 HTTP 响应返回
///
/// - `route_name`：JS Worker 路由名称
/// - req：原始 HTTP 请求
#[allow(clippy::too_many_lines)]
async fn handle_js_worker_route(
    route_name: String,
    req: axum::extract::Request,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    const ROUTE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;
    const ALLOWED_RESPONSE_HEADERS: &[&str] = &[
        "content-type",
        "content-length",
        "cache-control",
        "last-modified",
        "etag",
        "access-control-allow-origin",
        "access-control-allow-methods",
        "access-control-allow-headers",
    ];

    let route_name = {
        let trimmed = route_name.trim();
        if trimmed.len() == route_name.len() {
            route_name
        } else {
            trimmed.to_owned()
        }
    };
    if route_name.is_empty() {
        warn!(target: "js_worker", "route request with empty route_name");
        return build_http_error(StatusCode::BAD_REQUEST, "route_name cannot be empty");
    }

    // 提取客户端 IP
    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map_or_else(|| "127.0.0.1".to_owned(), |info| info.0.ip().to_string());

    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let uri = parts.uri.to_string();
    // 处理反向代理场景下的协议和主机名
    let scheme = parts
        .headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = parts
        .headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let url = if uri.starts_with("http://") || uri.starts_with("https://") {
        uri
    } else {
        format!("{scheme}://{host}{uri}")
    };

    // 收集请求头，过滤并注入 ng-connecting-ip
    let mut headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|v| JsRouteHeader {
                name: name.as_str().to_owned(),
                value: v.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    headers.retain(|h| !h.name.eq_ignore_ascii_case("ng-connecting-ip"));
    headers.push(JsRouteHeader {
        name: "ng-connecting-ip".to_owned(),
        value: peer_ip,
    });

    // 读取请求体（限制大小）
    let body_bytes = match axum::body::to_bytes(body, ROUTE_BODY_LIMIT_BYTES).await {
        Ok(bytes) => bytes.to_vec(),
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "failed to read request body");
            return build_http_error(
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {e}"),
            );
        }
    };

    let db = if let Some(db) = ng_db::get_db() {
        db.clone()
    } else {
        error!(target: "js_worker", route_name = %route_name, "DB not initialized for route request");
        return build_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database is not initialized",
        );
    };

    // 从数据库查询 route_name 绑定的 JS Worker
    let model = match js_worker::Entity::find()
        .filter(js_worker::Column::RouteName.eq(route_name.as_str()))
        .one(&db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => {
            warn!(target: "js_worker", route_name = %route_name, "no js_worker bound to route_name");
            return build_http_error(
                StatusCode::NOT_FOUND,
                "No js_worker bound to this route_name",
            );
        }
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "DB query failed for route request");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database query failed: {e}"),
            );
        }
    };

    let bytecode = match ensure_bytecode_version(&model, &db).await {
        Ok(bc) => bc,
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, worker_name = %model.name, error = %e, "js_worker bytecode version check/recompile failed");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("js_worker '{}' bytecode error: {e}", model.name),
            );
        }
    };

    // Base64 编码请求体（CPU 操作耗时微秒级，spawn_blocking 开销反而更大）
    let body_base64 = base64::engine::general_purpose::STANDARD.encode(&body_bytes);
    let js_input = JsRouteInput {
        method,
        url,
        headers,
        body_base64,
    };
    let params = match serde_json::to_value(js_input) {
        Ok(v) => v,
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "failed to serialize route input");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize route input: {e}"),
            );
        }
    };

    // 构建运行时限制参数
    let env = model.env.unwrap_or_else(|| serde_json::json!({}));
    let limits = RuntimeLimits::from_model(
        model.max_run_time,
        model.max_stack_size,
        model.max_heap_size,
    );
    let run_result = runtime_pool::global_pool()
        .execute_script(
            model.name.as_str(),
            bytecode,
            RunType::Route,
            params,
            env,
            model.runtime_clean_time,
            limits,
        )
        .await;

    let js_value = match run_result {
        Ok(v) => v,
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "route worker execution failed");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Route worker execution failed: {e}"),
            );
        }
    };

    // 解析 JS 脚本输出
    let js_output: JsRouteOutput = match serde_json::from_value(js_value) {
        Ok(v) => v,
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "invalid onRoute return format");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Invalid onRoute return format: {e}"),
            );
        }
    };

    // 构建响应：仅允许白名单内的响应头，防止 JS Worker 注入敏感头（Set-Cookie、Location、CSP 等）
    let status = StatusCode::from_u16(js_output.status).unwrap_or(StatusCode::OK);
    let mut response = axum::http::Response::builder().status(status);
    for header in js_output.headers {
        if let Ok(name) = axum::http::header::HeaderName::from_bytes(header.name.as_bytes())
            && let Ok(value) = axum::http::header::HeaderValue::from_str(header.value.as_str())
        {
            if !ALLOWED_RESPONSE_HEADERS
                .iter()
                .any(|&allowed| allowed.eq_ignore_ascii_case(name.as_str()))
            {
                continue;
            }
            response = response.header(name, value);
        }
    }

    // 解码 Base64 响应体
    let body_bytes = match base64::engine::general_purpose::STANDARD.decode(&js_output.body_base64)
    {
        Ok(v) => v,
        Err(e) => {
            error!(target: "js_worker", route_name = %route_name, error = %e, "failed to decode base64 body");
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to decode base64 body: {e}"),
            );
        }
    };

    response
        .body(jsonrpsee::server::HttpBody::from(body_bytes))
        .unwrap_or_else(|e| {
            error!(target: "js_worker", route_name = %route_name, error = %e, "failed to build HTTP response");
            build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build response: {e}"),
            )
        })
}

/// 根据文件扩展名猜测 MIME 类型
///
/// 覆盖常见 Web 静态资源类型，未匹配时返回 `application/octet-stream`。
fn guess_mime_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// 提供静态文件服务
///
/// - `sub_path`：静态文件桶的子路径
/// - path：请求 URI 路径
/// - cors：是否添加 CORS 允许头
/// - method：HTTP 方法（仅允许 GET/HEAD，其他返回 405）
///
/// 路径安全：使用 [`resolve_safe_file_path`] 防止路径遍历攻击。
/// 目录请求自动回落到 index.html。
async fn serve_static_file(
    sub_path: &str,
    path: &str,
    cors: bool,
    method: &axum::http::Method,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    // 仅允许 GET / HEAD；其它方法（包括非 CORS 预检的 OPTIONS）返回 405
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        let mut builder = axum::http::Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(axum::http::header::ALLOW, "GET, HEAD, OPTIONS");
        if cors {
            builder = builder.header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
        }
        return builder
            .body(jsonrpsee::server::HttpBody::from("Method not allowed"))
            .expect("Failed to build 405 response");
    }

    let static_path = get_static_path();
    let file_path = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    // 路径安全校验，防止目录遍历
    let resolved = match resolve_safe_file_path(&static_path, sub_path, file_path) {
        Ok(p) => p,
        Err(e) => return build_static_error(StatusCode::BAD_REQUEST, format!("{e}"), cors),
    };

    let data = match tokio::fs::read(&resolved).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 如果请求路径对应的是一个目录，自动返回该目录下的 index.html
            let index_html_path = resolved.join("index.html");
            if let Ok(d) = tokio::fs::read(&index_html_path).await {
                d
            } else {
                return build_static_error(StatusCode::NOT_FOUND, "File not found", cors);
            }
        }
        Err(e) => {
            return build_static_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read file: {e}"),
                cors,
            );
        }
    };

    let content_type = guess_mime_type(&resolved);
    let mut builder = axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, content_type);

    if cors {
        builder = builder.header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    }

    // HEAD 请求不返回 body
    let body = if method == axum::http::Method::HEAD {
        jsonrpsee::server::HttpBody::default()
    } else {
        jsonrpsee::server::HttpBody::from(data)
    };

    builder
        .body(body)
        .unwrap_or_else(|e| build_http_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))
}

/// 构建通用 HTTP 错误响应（text/plain）
fn build_http_error(
    status: StatusCode,
    message: impl Into<String>,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    axum::http::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .body(jsonrpsee::server::HttpBody::from(message.into()))
        .expect("Failed to build error response")
}

/// 构建静态文件服务专用错误响应：按需带上 CORS 头，便于浏览器读取错误信息
fn build_static_error(
    status: StatusCode,
    message: impl Into<String>,
    cors: bool,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    let mut builder = axum::http::Response::builder().status(status).header(
        axum::http::header::CONTENT_TYPE,
        "text/plain; charset=utf-8",
    );
    if cors {
        builder = builder.header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    }
    builder
        .body(jsonrpsee::server::HttpBody::from(message.into()))
        .expect("Failed to build error response")
}

/// 绑定 Unix Socket 监听器
///
/// 内部步骤：
/// 1. 创建父目录（如不存在）
/// 2. 删除已有的 socket 文件（避免地址已占用错误）
/// 3. 绑定新监听器
#[cfg(not(target_os = "windows"))]
async fn bind_unix_listener(path: &str) -> std::io::Result<tokio::net::UnixListener> {
    use std::io::ErrorKind;
    use std::path::Path;

    let socket_path = Path::new(path);
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    match tokio::fs::remove_file(socket_path).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    tokio::net::UnixListener::bind(socket_path)
}

/// 清理 Unix Socket 文件
///
/// 服务器关闭时调用，删除 socket 文件以避免残留。
#[cfg(not(target_os = "windows"))]
async fn cleanup_unix_socket_file(path: Option<&str>) {
    use std::io::ErrorKind;
    let Some(path) = path else { return };
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(target: "server", path = %path, error = %e, "Failed to remove unix socket file");
        }
    }
}

// ── Trait 注入的具体实现（依赖倒置）──────────────────────────

/// 统一权限校验实现：委托至 `ng_token::check_token_limit` / `check_super_token` / `get_token`
///
/// 替代原先的 ServerAuthProvider、KvTokenChecker、StaticTokenChecker、
/// TaskAuthProvider、JsWorkerTokenChecker、TerminalTokenChecker 六个重复实现。
struct ServerPermissionChecker;

impl ng_core::permission::permission_checker::PermissionChecker for ServerPermissionChecker {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send>> {
        let token_or_auth = token_or_auth.clone();
        Box::pin(
            async move { ng_token::check_token_limit(&token_or_auth, scopes, permissions).await },
        )
    }

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send>> {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::check_super_token(&token_or_auth).await })
    }

    fn get_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = anyhow::Result<ng_core::permission::data_structure::Token>,
                > + Send,
        >,
    > {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::get_token(&token_or_auth).await })
    }
}

/// ng-task `MonitoringUuidProvider` 实现：委托至 `ng_monitoring::MonitoringUuidCache`
struct TaskMonitoringUuidProvider;

impl ng_task::MonitoringUuidProvider for TaskMonitoringUuidProvider {
    fn get_or_insert(
        &self,
        uuid: uuid::Uuid,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<i16, ng_core::error::NodegetError>> + Send>,
    > {
        Box::pin(async move {
            ng_monitoring::monitoring_uuid_cache::MonitoringUuidCache::global()
                .expect("MonitoringUuidCache not initialized in TaskMonitoringUuidProvider")
                .get_or_insert(uuid)
                .await
        })
    }

    fn reload(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> {
        Box::pin(ng_monitoring::monitoring_uuid_cache::MonitoringUuidCache::reload())
    }
}

/// ng-js-runtime `JsWorkerService` 实现：委托至 `ng_js_worker::service`
///
/// 提供两个功能：
/// - `run_inline_call_and_record_result`：执行内联 JS 调用并记录结果
/// - `get_rpc_module`：获取 RPC 模块分发器，供 JS 脚本调用内部 RPC
struct JsWorkerServiceImpl;

impl ng_js_runtime::js_worker_service::JsWorkerService for JsWorkerServiceImpl {
    fn run_inline_call_and_record_result(
        &self,
        js_script_name: String,
        params_json: String,
        timeout_sec: Option<f64>,
        inline_caller: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
        Box::pin(async move {
            ng_js_worker::service::run_inline_call_and_record_result(
                js_script_name,
                params_json,
                timeout_sec,
                inline_caller,
            )
            .await
        })
    }

    fn get_rpc_module(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Box<dyn ng_js_runtime::js_worker_service::RawJsonDispatcher + Send>,
                > + Send,
        >,
    > {
        Box::pin(async move {
            let module = crate::rpc_nodeget::get_modules();
            Box::new(RpcModuleDispatcher(module))
                as Box<dyn ng_js_runtime::js_worker_service::RawJsonDispatcher + Send>
        })
    }
}

/// RPC 模块 JSON 分发器
///
/// 实现 `RawJsonDispatcher`，使 JS 脚本可通过原始 JSON 字符串调用任意 RPC 方法。
struct RpcModuleDispatcher(jsonrpsee::RpcModule<()>);

impl ng_js_runtime::js_worker_service::RawJsonDispatcher for RpcModuleDispatcher {
    fn raw_json_request(
        &self,
        json: &str,
        buf_size: usize,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<(String, ())>> + Send + '_>,
    > {
        let json = json.to_owned();
        let module = self.0.clone();
        Box::pin(async move {
            let (resp, _stream) = module
                .raw_json_request(&json, buf_size)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok((resp.to_string(), ()))
        })
    }
}

/// ng-crontab `JsWorkerScheduler` 实现：委托至 `ng_js_worker::service::enqueue_defined_js_worker_run`
///
/// 当 cron 任务触发时，将 JS Worker 执行任务入队。
struct CronJsWorkerScheduler;

impl ng_crontab::task::JsWorkerScheduler for CronJsWorkerScheduler {
    fn enqueue_run(
        &self,
        worker_name: String,
        run_type: ng_js_runtime::RunType,
        params: serde_json::Value,
        env_override: Option<serde_json::Value>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send>> {
        Box::pin(async move {
            ng_js_worker::service::enqueue_defined_js_worker_run(
                worker_name,
                run_type,
                params,
                env_override,
            )
            .await
        })
    }
}
