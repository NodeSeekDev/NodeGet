use axum::routing::any;
use tower::Service;

use crate::crontab::init_crontab_worker;
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

    let rpc_module = get_modules();

    let (stop_handle, _server_handle) = jsonrpsee::server::stop_channel();
    let rpc_middleware = jsonrpsee::server::middleware::rpc::RpcServiceBuilder::new()
        .layer_fn(move |service| RpcTimingMiddleware {
            service,
            level: rpc_timing_log_level,
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
        .build(rpc_module, stop_handle);

    let app = axum::Router::new()
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

    axum::serve(listener, app).await.unwrap();
}

#[cfg(all(not(target_os = "windows"), feature = "jemalloc"))]
fn spawn_jemalloc_mem_debug_task() {
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
