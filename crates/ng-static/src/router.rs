use axum::response::IntoResponse;
use axum::routing::any;
use axum::{extract::Path, http::StatusCode};
use base64::Engine as _;
use dav_server::{DavHandler, fakels::FakeLs, localfs::LocalFs};
use ng_core::permission::data_structure::{
    Permission, Scope, StaticBucketFile as StaticBucketFilePermission,
};
use ng_core::permission::token_auth::TokenOrAuth;
use tracing::{debug, error, info, warn};

use crate::auth::get_token_checker;
use crate::cache::StaticCache;
use crate::ops::{get_static_path, resolve_safe_file_path};

/// Build and return an axum Router for static file serving and WebDAV.
///
/// Routes:
/// - `/nodeget/static/{name}` — serve bucket root (index.html)
/// - `/nodeget/static/{name}/{*path}` — serve specific file
/// - `/nodeget/static-webdav/{*path}` — WebDAV access (requires Basic Auth)
pub fn router() -> axum::Router {
    axum::Router::new()
        .route(
            "/nodeget/static/{name}",
            any(
                |Path(name): Path<String>, req: axum::extract::Request| async move {
                    let cache = StaticCache::global();
                    let Some(model) = cache.get_by_name(&name) else {
                        return build_http_error(StatusCode::NOT_FOUND, "Static not found");
                    };
                    // enable == Some(false) 视为不存在，返回 404
                    if model.enable != Some(false) {
                        if req.method() == axum::http::Method::OPTIONS && model.cors {
                            return axum::http::Response::builder()
                                .status(StatusCode::NO_CONTENT)
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
                                .body(jsonrpsee::server::HttpBody::default())
                                .expect("Failed to build CORS response");
                        }
                        let method = req.method().clone();
                        serve_static_file(&model.path, "/", model.cors, &method).await
                    } else {
                        build_http_error(StatusCode::NOT_FOUND, "Static not found")
                    }
                },
            ),
        )
        .route(
            "/nodeget/static/{name}/{*path}",
            any(
                |Path((name, path)): Path<(String, String)>,
                 req: axum::extract::Request| async move {
                    let cache = StaticCache::global();
                    let Some(model) = cache.get_by_name(&name) else {
                        return build_http_error(StatusCode::NOT_FOUND, "Static not found");
                    };
                    // enable == Some(false) 视为不存在，返回 404
                    if model.enable != Some(false) {
                        // 处理 OPTIONS 预检请求
                        if req.method() == axum::http::Method::OPTIONS && model.cors {
                            return axum::http::Response::builder()
                                .status(StatusCode::NO_CONTENT)
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                                .header(axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
                                .body(jsonrpsee::server::HttpBody::default())
                                .expect("Failed to build CORS response");
                        }
                        let file_path = if path.is_empty() { "/".to_string() } else { path };
                        let method = req.method().clone();
                        serve_static_file(&model.path, &file_path, model.cors, &method).await
                    } else {
                        build_http_error(StatusCode::NOT_FOUND, "Static not found")
                    }
                },
            ),
        )
        // WebDAV routes for static bucket file management
        .route("/nodeget/static-webdav/{*path}", any(static_webdav_handler))
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

/// WebDAV handler for static buckets.
///
/// Route: `/nodeget/static-webdav/{name}[/{*path}]`
/// Auth: HTTP Basic Auth (username=tokenkey/username, password=tokensecret/password).
/// Permission: requires **all** `StaticBucketFile` permissions (Read/Write/Delete/List)
///             on the requested bucket scope.
async fn static_webdav_handler(req: axum::extract::Request) -> axum::response::Response {
    let method = req.method().clone();
    let uri_path = req.uri().path().to_owned();

    // 从 URI path 解析 bucket name，避免 Axum 多段路由 Path 提取器数量不匹配
    let relative = uri_path
        .strip_prefix("/nodeget/static-webdav/")
        .unwrap_or_else(|| uri_path.trim_start_matches('/'));
    let (name, _rest) = relative.split_once('/').unwrap_or((relative, ""));
    let name = name.trim_end_matches('/');

    if name.is_empty() {
        warn!(target: "webdav", method = %method, uri = %uri_path, "missing bucket name in webdav url");
        return build_webdav_error(StatusCode::NOT_FOUND, "Missing bucket name in WebDAV URL");
    }

    debug!(target: "webdav", method = %method, uri = %uri_path, bucket = %name, "webdav request received");

    // 1. Look up bucket
    let cache = StaticCache::global();
    let Some(model) = cache.get_by_name(name) else {
        warn!(target: "webdav", method = %method, uri = %uri_path, bucket = %name, "bucket not found");
        return build_webdav_error(StatusCode::NOT_FOUND, "Static bucket not found");
    };
    debug!(target: "webdav", bucket = %name, path = %model.path, cors = model.cors, "bucket resolved");

    // 2. Extract Basic Auth
    let Some(auth_header) = req.headers().get(axum::http::header::AUTHORIZATION) else {
        warn!(target: "webdav", bucket = %name, method = %method, "missing authorization header");
        return build_webdav_auth_required();
    };
    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => {
            warn!(target: "webdav", bucket = %name, "invalid authorization header encoding");
            return build_webdav_auth_required();
        }
    };
    if !auth_str.starts_with("Basic ") {
        warn!(target: "webdav", bucket = %name, "authorization header not basic");
        return build_webdav_auth_required();
    }
    let credentials = match base64::engine::general_purpose::STANDARD.decode(&auth_str[6..]) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                warn!(target: "webdav", bucket = %name, "invalid base64 or non-utf8 credentials");
                return build_webdav_auth_required();
            }
        },
        Err(_) => {
            warn!(target: "webdav", bucket = %name, "invalid base64 in authorization header");
            return build_webdav_auth_required();
        }
    };
    let (username, password) = credentials.split_once(':').unwrap_or((&credentials, ""));

    // 3. Validate token
    let full_token = format!("{username}:{password}");
    let token_or_auth = match TokenOrAuth::from_full_token(&full_token) {
        Ok(t) => t,
        Err(_) => {
            let full_auth = format!("{username}|{password}");
            match TokenOrAuth::from_full_token(&full_auth) {
                Ok(t) => t,
                Err(_) => {
                    warn!(target: "webdav", bucket = %name, username = %username, "token/auth parse failed");
                    return build_webdav_auth_required();
                }
            }
        }
    };
    debug!(target: "webdav", bucket = %name, username = %username, "token parsed successfully");

    // 4. Check all StaticBucketFile permissions
    let permissions = vec![
        Permission::StaticBucketFile(StaticBucketFilePermission::Read),
        Permission::StaticBucketFile(StaticBucketFilePermission::Write),
        Permission::StaticBucketFile(StaticBucketFilePermission::Delete),
        Permission::StaticBucketFile(StaticBucketFilePermission::List),
    ];
    let is_allowed = match get_token_checker()
        .check_token_limit(
            &token_or_auth,
            vec![Scope::StaticBucket(name.to_string())],
            permissions,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            error!(target: "webdav", bucket = %name, username = %username, error = %e, "permission check failed internally");
            false
        }
    };

    if !is_allowed {
        warn!(target: "webdav", bucket = %name, username = %username, "insufficient permissions");
        return build_webdav_error(
            StatusCode::FORBIDDEN,
            "Forbidden: insufficient StaticBucketFile permissions",
        );
    }
    debug!(target: "webdav", bucket = %name, username = %username, "all permissions granted");

    // 5. Serve via WebDAV
    let static_path = get_static_path();
    let disk_path = std::path::PathBuf::from(&static_path).join(&model.path);

    info!(target: "webdav", bucket = %name, username = %username, disk_path = %disk_path.display(), method = %method, "serving webdav request");

    let dav = DavHandler::builder()
        .filesystem(LocalFs::new(&disk_path, false, false, false))
        .locksystem(FakeLs::new())
        .strip_prefix(format!("/nodeget/static-webdav/{}", name))
        .build_handler();

    let resp = dav.handle(req).await.into_response();
    let status = resp.status();
    if status.is_success() || status.is_redirection() || status == StatusCode::NOT_MODIFIED {
        debug!(target: "webdav", bucket = %name, username = %username, status = %status, "webdav request completed");
    } else {
        warn!(target: "webdav", bucket = %name, username = %username, status = %status, "webdav request returned non-success status");
    }
    resp
}

fn build_webdav_auth_required() -> axum::response::Response {
    axum::http::Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            axum::http::header::WWW_AUTHENTICATE,
            "Basic realm=\"NodeGet Static WebDAV\"",
        )
        .body(axum::body::Body::from("Authentication required"))
        .expect("Failed to build response")
}

fn build_webdav_error(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    axum::http::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .body(axum::body::Body::from(message.into()))
        .expect("Failed to build response")
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
