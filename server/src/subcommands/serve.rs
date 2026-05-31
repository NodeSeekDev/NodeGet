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
use ng_static::cache::StaticCache;
use ng_static::ops::{get_static_path, resolve_safe_file_path};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower::Service;
use tracing::{debug, error, info, warn};

use crate::rpc_nodeget::get_modules;
use crate::rpc_timing::RpcTimingMiddleware;

pub async fn run(config: &ServerConfig) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    super::init_or_skip_super_token().await;
    debug!(target: "server", "Super token initialization completed");

    // ── 初始化各缓存 ──────────────────────────────────────────────
    ng_token::TokenCache::init()
        .await
        .expect("Failed to initialize token cache");
    debug!(target: "server", "Token cache initialized");

    // 注册 auth checker（TokenAuthChecker → ng-infra 全局）
    ng_token::register_auth_checker();
    debug!(target: "server", "Auth checker registered");

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
    // ng-config: 注册 super token 验证函数
    ng_config::server_rpc::register_check_super_token(|token_or_auth| {
        Box::pin(async move { ng_token::check_super_token(token_or_auth).await })
    });
    debug!(target: "server", "ng-config check_super_token registered");

    // ng-db: 注册 auth provider
    ng_db::rpc::set_auth_provider(std::sync::Arc::new(ServerAuthProvider));
    debug!(target: "server", "ng-db auth provider registered");

    // ng-kv: 注册 token permission checker
    ng_kv::set_token_checker(Box::new(KvTokenChecker));
    debug!(target: "server", "ng-kv token checker registered");

    // ng-static: 注册 token permission checker
    ng_static::auth::set_token_checker(Box::new(StaticTokenChecker));
    debug!(target: "server", "ng-static token checker registered");

    // ng-task: 注册 auth provider + monitoring UUID provider
    ng_task::set_auth_provider(std::sync::Arc::new(TaskAuthProvider));
    ng_task::set_monitoring_uuid_provider(std::sync::Arc::new(TaskMonitoringUuidProvider));
    debug!(target: "server", "ng-task providers registered");

    // ng-js-worker: 注册 token permission checker
    ng_js_worker::set_token_checker(Box::new(JsWorkerTokenChecker));
    debug!(target: "server", "ng-js-worker token checker registered");

    // ng-terminal: 注册 token permission checker
    ng_terminal::set_token_checker(Box::new(TerminalTokenChecker));
    debug!(target: "server", "ng-terminal token checker registered");

    // ng-js-runtime: 注册 JsWorkerService (inline_call + nodeget RPC dispatch)
    ng_js_runtime::js_worker_service::set_js_worker_service(Box::new(JsWorkerServiceImpl));
    debug!(target: "server", "ng-js-runtime JsWorkerService registered");

    // ng-crontab: 注册 JsWorkerScheduler (cron 触发 JS Worker 任务)
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
                            let cache = StaticCache::global();
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

                        rpc_service.call(req).await.unwrap()
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
                            let cache = StaticCache::global();
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

                        rpc_service.call(req).await.unwrap()
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
                        return rpc_service.call(req).await.unwrap();
                    }
                    let cache = StaticCache::global();
                    if let Some(model) = cache.get_http_root() {
                        let path = req.uri().path().to_owned();
                        let method = req.method().clone();
                        return serve_static_file(&model.path, &path, model.cors, &method).await;
                    }
                    rpc_service.call(req).await.unwrap()
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
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .unwrap_or_else(|e| panic!("Failed to load TLS config: {e}"));
        let serve_future =
            axum_server::bind_rustls(addr, tls_config).serve(app.into_make_service());
        tokio::pin!(serve_future);

        tokio::select! {
            result = &mut serve_future => {
                result.unwrap();
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
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
            result = &mut serve_future => {
                result.unwrap();
                ng_monitoring::monitoring_buffer::flush_and_shutdown().await;
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

#[derive(Debug, Serialize)]
struct JsRouteHeader {
    name: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct JsRouteInput {
    method: String,
    url: String,
    headers: Vec<JsRouteHeader>,
    body_base64: String,
}

#[derive(Debug, Deserialize)]
struct JsRouteOutput {
    status: u16,
    headers: Vec<JsRouteOutputHeader>,
    body_base64: String,
}

#[derive(Debug, Deserialize)]
struct JsRouteOutputHeader {
    name: String,
    value: String,
}

async fn handle_js_worker_route(
    route_name: String,
    req: axum::extract::Request,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    const ROUTE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

    let route_name = route_name.trim().to_owned();
    if route_name.is_empty() {
        warn!(target: "js_worker", "route request with empty route_name");
        return build_http_error(StatusCode::BAD_REQUEST, "route_name cannot be empty");
    }

    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map_or_else(|| "127.0.0.1".to_owned(), |info| info.0.ip().to_string());

    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let uri = parts.uri.to_string();
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

    let Some(bytecode) = model.js_byte_code else {
        error!(target: "js_worker", route_name = %route_name, worker_name = %model.name, "js_worker has no precompiled bytecode");
        return build_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("js_worker '{}' has no precompiled bytecode", model.name),
        );
    };

    let body_base64 = tokio::task::spawn_blocking(move || {
        base64::engine::general_purpose::STANDARD.encode(&body_bytes)
    })
    .await
    .unwrap_or_else(|e| {
        error!(target: "js_worker", route_name = %route_name, error = %e, "base64 encoding task panicked");
        String::new()
    });
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

    let env = model.env.unwrap_or_else(|| serde_json::json!({}));
    let limits = RuntimeLimits::from_model(
        model.max_run_time,
        model.max_stack_size,
        model.max_heap_size,
    );
    let run_result = runtime_pool::init_global_pool()
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

    let status = StatusCode::from_u16(js_output.status).unwrap_or(StatusCode::OK);
    let mut response = axum::http::Response::builder().status(status);
    for header in js_output.headers {
        if let Ok(name) = axum::http::header::HeaderName::from_bytes(header.name.as_bytes())
            && let Ok(value) = axum::http::header::HeaderValue::from_str(header.value.as_str())
        {
            if name == "content-encoding" || name == "transfer-encoding" {
                continue;
            }
            response = response.header(name, value);
        }
    }

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

/// 静态文件服务专用的错误响应：按需带上 CORS 头，便于浏览器读取错误信息
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

// ── Trait implementations for dependency injection ──────────────────

/// ng-db auth provider: delegates to `ng_token::check_token_limit` / `check_super_token`
struct ServerAuthProvider;

impl ng_db::rpc::AuthProvider for ServerAuthProvider {
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
}

/// ng-kv token permission checker: delegates to `ng_token::check_token_limit` / `check_super_token` / `get_token`
struct KvTokenChecker;

impl ng_kv::TokenPermissionChecker for KvTokenChecker {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(
            async move { ng_token::check_token_limit(&token_or_auth, scopes, permissions).await },
        )
    }

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
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
                > + Send
                + '_,
        >,
    > {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::get_token(&token_or_auth).await })
    }
}

/// ng-static token permission checker: delegates to `ng_token::check_token_limit` / `check_super_token`
struct StaticTokenChecker;

impl ng_static::auth::TokenPermissionChecker for StaticTokenChecker {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(
            async move { ng_token::check_token_limit(&token_or_auth, scopes, permissions).await },
        )
    }

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::check_super_token(&token_or_auth).await })
    }
}

/// ng-task auth provider: delegates to `ng_token::check_token_limit` / `check_super_token` / `get_token`
struct TaskAuthProvider;

impl ng_task::TaskAuthProvider for TaskAuthProvider {
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

/// ng-task monitoring UUID provider: delegates to `ng_monitoring::MonitoringUuidCache`
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

/// ng-js-worker token permission checker: delegates to `ng_token::check_token_limit` / `check_super_token` / `get_token`
struct JsWorkerTokenChecker;

impl ng_js_worker::TokenPermissionChecker for JsWorkerTokenChecker {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(
            async move { ng_token::check_token_limit(&token_or_auth, scopes, permissions).await },
        )
    }

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
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
                > + Send
                + '_,
        >,
    > {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::get_token(&token_or_auth).await })
    }
}

/// ng-terminal token permission checker: delegates to `ng_token::check_token_limit` / `check_super_token`
struct TerminalTokenChecker;

impl ng_terminal::TokenPermissionChecker for TerminalTokenChecker {
    fn check_token_limit(
        &self,
        token_or_auth: &TokenOrAuth,
        scopes: Vec<Scope>,
        permissions: Vec<Permission>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(
            async move { ng_token::check_token_limit(&token_or_auth, scopes, permissions).await },
        )
    }

    fn check_super_token(
        &self,
        token_or_auth: &TokenOrAuth,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let token_or_auth = token_or_auth.clone();
        Box::pin(async move { ng_token::check_super_token(&token_or_auth).await })
    }
}

struct JsWorkerServiceImpl;

impl ng_js_runtime::js_worker_service::JsWorkerService for JsWorkerServiceImpl {
    fn run_inline_call_and_record_result(
        &self,
        js_script_name: String,
        params: serde_json::Value,
        timeout_sec: Option<f64>,
        inline_caller: Option<String>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<serde_json::Value>> + Send>,
    > {
        Box::pin(async move {
            ng_js_worker::service::run_inline_call_and_record_result(
                js_script_name,
                params,
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

/// ng-crontab `JsWorkerScheduler`: delegates to `ng_js_worker::service::enqueue_defined_js_worker_run`
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
