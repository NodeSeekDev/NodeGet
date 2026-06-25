//! 静态文件 HTTP 路由与 WebDAV 处理模块。
//!
//! 职责：构建 axum Router，提供静态文件 HTTP 服务和 WebDAV 协议支持。
//!
//! 路由：
//! - `/nodeget/static/{name}` — 桶根路径（默认返回 index.html）
//! - `/nodeget/static/{name}/{*path}` — 桶内具体文件
//! - `/nodeget/static-webdav/{*path}` — WebDAV 访问（需 Basic Auth，要求全部四种权限）
//!
//! 协作关系：从 [`StaticCache`] 查询桶配置，通过 [`resolve_safe_file_path`]
//! 确保磁盘路径安全；DavHandler 按 bucket 名称缓存以避免每次请求重建。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

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

use crate::cache::StaticCache;
use crate::ops::{get_static_path, resolve_safe_file_path};

/// Global cache of DavHandler instances keyed by bucket name.
/// Avoids re-allocating LocalFs, FakeLs, and DavHandler config on every WebDAV request.
static DAV_HANDLER_CACHE: OnceLock<RwLock<HashMap<String, DavHandler>>> = OnceLock::new();

fn dav_handler_cache() -> &'static RwLock<HashMap<String, DavHandler>> {
    DAV_HANDLER_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Clear all cached DavHandler instances.
///
/// Should be called when `static_path` changes (e.g. config hot-reload),
/// since cached handlers embed the old disk path.
pub fn clear_dav_handler_cache() {
    if let Some(cache) = DAV_HANDLER_CACHE.get() {
        cache
            .write()
            .expect("DAV handler cache lock poisoned")
            .clear();
    }
}

/// Retrieve a cached DavHandler for the given bucket, or build and cache one.
fn get_or_create_dav_handler(bucket_name: &str, disk_path: &std::path::Path) -> DavHandler {
    // Fast path: read lock
    if let Ok(cache) = dav_handler_cache().read()
        && let Some(handler) = cache.get(bucket_name)
    {
        return handler.clone();
    }
    // Slow path: write lock
    let mut cache = dav_handler_cache()
        .write()
        .expect("DAV handler cache lock poisoned");
    // Double-check after acquiring write lock
    if let Some(handler) = cache.get(bucket_name) {
        return handler.clone();
    }
    let handler = DavHandler::builder()
        .filesystem(LocalFs::new(disk_path, false, false, false))
        .locksystem(FakeLs::new())
        .strip_prefix(format!("/nodeget/static-webdav/{bucket_name}"))
        .build_handler();
    cache.insert(bucket_name.to_owned(), handler.clone());
    handler
}

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
                    let Some(cache) = StaticCache::global() else {
                        return build_http_error(StatusCode::INTERNAL_SERVER_ERROR, "StaticCache not initialized");
                    };
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
                        let if_none_match = header_str(req.headers().get(axum::http::header::IF_NONE_MATCH));
                        serve_static_file(&model.path, "/", model.cors, &method, if_none_match).await
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
                    let Some(cache) = StaticCache::global() else {
                        return build_http_error(StatusCode::INTERNAL_SERVER_ERROR, "StaticCache not initialized");
                    };
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
                        let if_none_match = header_str(req.headers().get(axum::http::header::IF_NONE_MATCH));
                        serve_static_file(&model.path, &file_path, model.cors, &method, if_none_match).await
                    } else {
                        build_http_error(StatusCode::NOT_FOUND, "Static not found")
                    }
                },
            ),
        )
        // WebDAV routes for static bucket file management
        .route("/nodeget/static-webdav/{*path}", any(static_webdav_handler))
}

/// 根据文件扩展名推断 MIME 类型，覆盖常见 Web 静态资源类型。
///
/// 未匹配时回退到 `application/octet-stream`。
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

/// 处理静态文件的 HTTP 请求（GET / HEAD / OPTIONS）。
///
/// - `sub_path` - 桶的磁盘子目录（对应数据库 `path` 字段）
/// - `path` - 请求的文件相对路径
/// - `cors` - 是否添加 CORS 响应头
/// - `method` - HTTP 方法
///
/// 返回：包含文件内容的 HTTP 响应，或对应的错误响应。
///
/// 内部步骤：
/// 1. 仅允许 GET / HEAD，其它方法返回 405
/// 2. 通过 [`resolve_safe_file_path`] 解析安全磁盘路径
/// 3. 读取文件，若路径对应目录则自动返回该目录下的 index.html
/// 4. 推断 MIME 类型，按需附加 CORS 头
/// 5. HEAD 请求不返回 body
async fn serve_static_file(
    sub_path: &str,
    path: &str,
    cors: bool,
    method: &axum::http::Method,
    if_none_match: Option<&str>,
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

    // 取文件元数据用于生成 ETag；若目标不存在但对应目录，回退到目录下的 index.html
    let (resolved, metadata) = match tokio::fs::metadata(&resolved).await {
        Ok(m) => (resolved.clone(), m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 目录回退：尝试 {resolved}/index.html
            let index_html_path = resolved.join("index.html");
            match tokio::fs::metadata(&index_html_path).await {
                Ok(m) => (index_html_path, m),
                Err(_) => return build_static_error(StatusCode::NOT_FOUND, "File not found", cors),
            }
        }
        Err(e) => {
            return build_static_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to stat file: {e}"),
                cors,
            );
        }
    };

    // ETag = 修改时间(秒) + 字节数，弱验证器。仅用于缓存命中判断，非内容哈希。
    let etag = build_etag(&metadata);

    // If-None-Match 命中：返回 304，不读文件内容（省带宽 + 读盘）
    if if_none_match_is_match(if_none_match, &etag) {
        let mut builder = axum::http::Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(axum::http::header::ETAG, etag.as_str())
            .header("X-Content-Type-Options", "nosniff");
        if cors {
            builder = builder.header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
        }
        return builder
            .body(jsonrpsee::server::HttpBody::default())
            .expect("Failed to build 304 response");
    }

    let data = match tokio::fs::read(&resolved).await {
        Ok(d) => d,
        Err(_) => {
            return build_static_error(StatusCode::NOT_FOUND, "File not found", cors);
        }
    };

    let content_type = guess_mime_type(&resolved);
    let mut builder = axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(axum::http::header::ETAG, etag.as_str())
        // 防止浏览器对响应做 MIME 嗅探,强制按 Content-Type 解释(配合用户上传内容,
        // 避免 octet-stream 被嗅探成可执行类型)。
        .header("X-Content-Type-Options", "nosniff");

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

/// 将 HeaderValue 安全转 `&str`（失败返回 None）。
fn header_str(value: Option<&axum::http::HeaderValue>) -> Option<&str> {
    value.and_then(|v| v.to_str().ok())
}

/// 根据文件元数据生成 ETag（弱验证器）。
///
/// 格式 `"<mtime_secs>-<size>"`：修改时间秒 + 字节数。文件内容变更时
/// mtime/size 通常变化，足以触发客户端重新获取；不保证内容级唯一性。
fn build_etag(metadata: &std::fs::Metadata) -> String {
    let mtime_secs = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("\"{}-{}\"", mtime_secs, metadata.len())
}

/// 判断客户端的 If-None-Match 头是否与当前 ETag 匹配（命中则返回 304）。
///
/// 支持 `*`（匹配任意）、单个 ETag、以及逗号分隔的多个 ETag；忽略弱验证器前缀 `W/`。
fn if_none_match_is_match(if_none_match: Option<&str>, etag: &str) -> bool {
    let Some(header) = if_none_match else {
        return false;
    };
    if header.trim() == "*" {
        return true;
    }
    header.split(',').any(|part| {
        let p = part.trim();
        let p = p.strip_prefix("W/").unwrap_or(p);
        p == etag
    })
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
    let Some(cache) = StaticCache::global() else {
        return build_webdav_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "StaticCache not initialized",
        );
    };
    let Some(model) = cache.get_by_name(name) else {
        warn!(target: "webdav", method = %method, uri = %uri_path, bucket = %name, "bucket not found");
        return build_webdav_error(StatusCode::NOT_FOUND, "Static bucket not found");
    };
    // 与 HTTP GET 路径一致：enable == Some(false) 视为桶已下线，拒绝访问。
    // 否则会出现「HTTP 已 404 但 WebDAV 仍开放读写删」的访问控制不一致。
    if model.enable == Some(false) {
        debug!(target: "webdav", method = %method, uri = %uri_path, bucket = %name, "bucket disabled, treating as not found");
        return build_webdav_error(StatusCode::NOT_FOUND, "Static bucket not found");
    }
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
    let permissions = [
        Permission::StaticBucketFile(StaticBucketFilePermission::Read),
        Permission::StaticBucketFile(StaticBucketFilePermission::Write),
        Permission::StaticBucketFile(StaticBucketFilePermission::Delete),
        Permission::StaticBucketFile(StaticBucketFilePermission::List),
    ];
    let is_allowed = match ng_core::permission::permission_checker::get_permission_checker() {
        Some(checker) => {
            match checker
                .check_token_limit(
                    &token_or_auth,
                    &[Scope::StaticBucket(name.to_string())],
                    &permissions,
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    error!(target: "webdav", bucket = %name, username = %username, error = %e, "permission check failed internally");
                    false
                }
            }
        }
        None => {
            error!(target: "webdav", bucket = %name, "PermissionChecker not initialized");
            return build_webdav_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PermissionChecker not initialized",
            );
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

    let dav = get_or_create_dav_handler(name, &disk_path);

    let resp = dav.handle(req).await.into_response();
    let status = resp.status();
    if status.is_success() || status.is_redirection() || status == StatusCode::NOT_MODIFIED {
        debug!(target: "webdav", bucket = %name, username = %username, status = %status, "webdav request completed");
    } else {
        warn!(target: "webdav", bucket = %name, username = %username, status = %status, "webdav request returned non-success status");
    }
    resp
}

/// 构建 401 Unauthorized 响应，附带 Basic Auth 质询头。
///
/// 用于 WebDAV 请求缺少或校验失败 Authorization 头时。
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

/// 构建 WebDAV 错误响应（纯文本 body，不附带 CORS 头）。
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

/// 构建 HTTP 错误响应（纯文本 body，不附带 CORS 头）。
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
