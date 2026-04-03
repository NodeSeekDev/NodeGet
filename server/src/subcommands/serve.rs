use crate::entity::js_worker;
use axum::routing::any;
use axum::{extract::Path, http::StatusCode};
use log::info;
use nodeget_lib::js_runtime::RunType;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower::Service;

use crate::RELOAD_NOTIFY;
use crate::crontab::init_crontab_worker;
use crate::js_runtime::runtime_pool;
use crate::rpc::get_modules;
use crate::rpc_timing::RpcTimingMiddleware;

pub async fn run(
    config: &nodeget_lib::config::server::ServerConfig,
    rpc_timing_log_level: log::Level,
) {
    #[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
    spawn_jemalloc_mem_debug_task();

    super::init_or_skip_super_token().await;

    let _ = nodeget_lib::utils::uuid::compare_uuid(config.server_uuid);

    let terminal_state = crate::terminal::TerminalState {
        sessions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    };

    runtime_pool::init_global_pool();

    let rpc_module = get_modules();

    let (stop_handle, _server_handle) = jsonrpsee::server::stop_channel();
    let rpc_middleware =
        jsonrpsee::server::middleware::rpc::RpcServiceBuilder::new().layer_fn(move |service| {
            RpcTimingMiddleware {
                service,
                level: rpc_timing_log_level,
            }
        });

    let jsonrpc_service = jsonrpsee::server::Server::builder()
        .set_rpc_middleware(rpc_middleware)
        .set_config(
            jsonrpsee::server::ServerConfig::builder()
                .max_connections(config.jsonrpc_max_connections.unwrap_or(100))
                .max_response_body_size(u32::MAX)
                .max_request_body_size(u32::MAX)
                .build(),
        )
        .to_service_builder()
        .build(rpc_module, stop_handle.clone());
    let jsonrpc_service_for_root = jsonrpc_service.clone();
    let landing_html = render_root_html(&config.server_uuid.to_string(), env!("CARGO_PKG_VERSION"));

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
                            return axum::response::Response::builder()
                                .status(axum::http::StatusCode::OK)
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
                "/worker-route/{route_name}",
                any(|Path(route_name): Path<String>, req: axum::extract::Request| async move {
                    handle_js_worker_route(route_name, req).await
                }),
            )
            .route(
                "/worker-route/{route_name}/",
                any(|Path(route_name): Path<String>, req: axum::extract::Request| async move {
                    handle_js_worker_route(route_name, req).await
                }),
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
            .route("/terminal", any(crate::terminal::terminal_ws_handler))
            .with_state(terminal_state)
            .fallback(any(move |req: axum::extract::Request| {
                let mut rpc_service = jsonrpc_service.clone();
                async move { rpc_service.call(req).await.unwrap() }
            }));

    init_crontab_worker();

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
                    if let Err(e) = axum::serve(unix_listener, unix_app.into_make_service()).await
                    {
                        log::error!("Unix socket server stopped with error: {e}");
                    }
                }));
                info!("Unix socket listener started: {socket_path}");
            }
            Err(e) => {
                log::error!("Failed to bind unix socket listener: {e}");
            }
        }
    }

    let listener =
        tokio::net::TcpListener::bind(config.ws_listener.parse::<std::net::SocketAddr>().unwrap())
            .await
            .unwrap();

    let serve_future = std::future::IntoFuture::into_future(axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    ));
    tokio::pin!(serve_future);

    tokio::select! {
        result = &mut serve_future => {
            result.unwrap();
            #[cfg(not(target_os = "windows"))]
            if let Some(task) = unix_server_task.take() {
                task.abort();
            }
            #[cfg(not(target_os = "windows"))]
            cleanup_unix_socket_file(unix_socket_path.as_deref()).await;
        }
        () = RELOAD_NOTIFY
            .get()
            .expect("Reload notify not initialized")
            .notified() => {
            info!("Config reload requested, stopping server for restart...");
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
    body_bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct JsRouteOutputHeader {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct JsRouteOutput {
    status: u16,
    headers: Vec<JsRouteOutputHeader>,
    body_bytes: Vec<u8>,
}

async fn handle_js_worker_route(
    route_name: String,
    req: axum::extract::Request,
) -> axum::http::Response<jsonrpsee::server::HttpBody> {
    const ROUTE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

    let route_name = route_name.trim().to_owned();
    if route_name.is_empty() {
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
            return build_http_error(
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {e}"),
            );
        }
    };

    let db = match crate::DB.get() {
        Some(db) => db.clone(),
        None => {
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database is not initialized",
            );
        }
    };

    let model = match js_worker::Entity::find()
        .filter(js_worker::Column::RouteName.eq(route_name.as_str()))
        .one(&db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => {
            return build_http_error(
                StatusCode::NOT_FOUND,
                "No js_worker bound to this route_name",
            );
        }
        Err(e) => {
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database query failed: {e}"),
            );
        }
    };

    let Some(bytecode) = model.js_byte_code else {
        return build_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("js_worker '{}' has no precompiled bytecode", model.name),
        );
    };

    let js_input = JsRouteInput {
        method,
        url,
        headers,
        body_bytes,
    };
    let params = match serde_json::to_value(js_input) {
        Ok(v) => v,
        Err(e) => {
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize route input: {e}"),
            );
        }
    };

    let env = model.env.unwrap_or_else(|| serde_json::json!({}));
    let run_result = crate::js_runtime::runtime_pool::init_global_pool()
        .execute_script(
            model.name.as_str(),
            bytecode,
            RunType::Route,
            params,
            env,
            model.runtime_clean_time,
        )
        .await;

    let js_value = match run_result {
        Ok(v) => v,
        Err(e) => {
            return build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Route worker execution failed: {e}"),
            );
        }
    };

    let js_output: JsRouteOutput = match serde_json::from_value(js_value) {
        Ok(v) => v,
        Err(e) => {
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
            response = response.header(name, value);
        }
    }

    response
        .body(jsonrpsee::server::HttpBody::from(js_output.body_bytes))
        .unwrap_or_else(|e| {
            build_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build response: {e}"),
            )
        })
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
        Err(e) => log::warn!("Failed to remove unix socket file '{path}': {e}"),
    }
}

#[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
fn spawn_jemalloc_mem_debug_task() {
    static JEMALLOC_MEM_DEBUG_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if JEMALLOC_MEM_DEBUG_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async {
        loop {
            use tikv_jemalloc_ctl::{epoch, stats};
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if epoch::advance().is_err() {
                return;
            }

            let allocated = stats::allocated::read().unwrap();
            let active = stats::active::read().unwrap();
            let resident = stats::resident::read().unwrap();
            let mapped = stats::mapped::read().unwrap();

            log::info!(
                "MEM STATS (Jemalloc Only): App Logic: {:.2} MB | Allocator Active: {:.2} MB | RSS (Resident): {:.2} MB | Mapped: {:.2} MB",
                allocated as f64 / 1024.0 / 1024.0,
                active as f64 / 1024.0 / 1024.0,
                resident as f64 / 1024.0 / 1024.0,
                mapped as f64 / 1024.0 / 1024.0
            );
        }
    });
}
