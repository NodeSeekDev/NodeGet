use axum::routing::any;
use log::info;
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

    let app = axum::Router::new()
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
                            .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                            .body(jsonrpsee::server::HttpBody::from(landing_html))
                            .expect("Failed to build HTML response");
                    }

                    rpc_service.call(req).await.unwrap()
                }
            }),
        )
        .route("/terminal", any(crate::terminal::terminal_ws_handler))
        .with_state(terminal_state)
        .fallback(any(move |req: axum::extract::Request| {
            let mut rpc_service = jsonrpc_service.clone();
            async move { rpc_service.call(req).await.unwrap() }
        }));

    init_crontab_worker();

    let listener =
        tokio::net::TcpListener::bind(config.ws_listener.parse::<std::net::SocketAddr>().unwrap())
            .await
            .unwrap();

    let serve_future = std::future::IntoFuture::into_future(axum::serve(listener, app));
    tokio::pin!(serve_future);

    tokio::select! {
        result = &mut serve_future => {
            result.unwrap();
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
