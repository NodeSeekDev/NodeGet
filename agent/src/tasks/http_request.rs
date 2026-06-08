//! HTTP 请求任务模块。
//!
//! 执行 Server 下发的 HTTP 请求任务，支持自定义方法、头部、请求体（纯文本或 Base64 编码）、
//! 指定出站 IP 族（IPv4/IPv6），并将响应状态码、头部和请求体（纯文本或 Base64）上报。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use log::warn;
use ng_core::error::NodegetError;
use ng_task::{HttpRequestTask, HttpRequestTaskResult};
use reqwest::{Client, Method};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;
use std::time::Duration;

/// HTTP 请求结果类型
pub type Result<T> = anyhow::Result<T>;

/// rustls crypto provider 初始化标记，防止重复安装。
static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();
/// 默认 reqwest Client 缓存（无 IP 绑定），避免每次请求重建连接池/TLS/DNS 缓存。
static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();
/// HTTP 请求超时时间，30 秒。
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 确保 rustls ring crypto provider 已安装（幂等）。
fn ensure_rustls_ring_provider() {
    let () = RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// 获取默认的缓存 reqwest Client（无 IP 绑定）。
///
/// `Client::clone()` 仅增加 Arc 引用计数，不复制连接池。
fn get_default_client() -> Client {
    DEFAULT_CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(HTTP_REQUEST_TIMEOUT)
                .build()
                .expect("Failed to build default HTTP client")
        })
        .clone()
}

/// 执行 HTTP 请求任务。
///
/// - `task` - HTTP 请求任务参数
///
/// 1. 初始化 rustls provider
/// 2. 解析 HTTP 方法和出站 IP 族
/// 3. 构建请求（含头部和请求体）
/// 4. 发送请求并收集响应
/// 5. 响应头部值非 ASCII 时 Base64 编码，响应体同理
///
/// 返回 [`HttpRequestTaskResult`]；请求失败时返回错误。
pub async fn execute_http_request(task: HttpRequestTask) -> Result<HttpRequestTaskResult> {
    ensure_rustls_ring_provider();

    let method =
        Method::from_bytes(task.method.trim().to_ascii_uppercase().as_bytes()).map_err(|e| {
            warn!(target: "task", "HTTP 请求方法无效: method={}, error={e}", task.method);
            NodegetError::InvalidInput(format!("Invalid http_request.method: {e}"))
        })?;

    let client = if let Some(ip_raw) = task.ip.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let ip = match ip_raw.to_ascii_lowercase().as_str() {
            "ipv4 auto" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            "ipv6 auto" => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            _ => ip_raw.parse().map_err(|e| {
                warn!(target: "task", "HTTP 请求 IP 无效: ip='{ip_raw}', error={e}");
                NodegetError::InvalidInput(format!("Invalid http_request.ip '{ip_raw}': {e}"))
            })?,
        };
        Client::builder()
            .timeout(HTTP_REQUEST_TIMEOUT)
            .local_address(ip)
            .build()
            .map_err(|e| {
                warn!(target: "task", "HTTP 客户端构建失败: error={e}");
                NodegetError::Other(format!("Failed to build HTTP client: {e}"))
            })?
    } else {
        get_default_client()
    };

    let url_str = task.url.to_string();
    let mut req = client.request(method, task.url);
    for (k, v) in &task.headers {
        req = req.header(k, v);
    }

    match (&task.body, &task.body_base64) {
        (Some(_), Some(_)) => {
            warn!(target: "task", "HTTP 请求参数冲突: body 和 body_base64 不能同时指定");
            return Err(NodegetError::InvalidInput(
                "http_request.body and http_request.body_base64 are mutually exclusive".to_owned(),
            )
            .into());
        }
        (Some(body), None) => {
            req = req.body(body.clone());
        }
        (None, Some(b64)) => {
            let bytes = BASE64_STANDARD.decode(b64).map_err(|e| {
                warn!(target: "task", "HTTP 请求 body_base64 解码失败: error={e}");
                NodegetError::InvalidInput(format!("Invalid http_request.body_base64: {e}"))
            })?;
            req = req.body(bytes);
        }
        (None, None) => {}
    }

    let resp = req.send().await.map_err(|e| {
        warn!(target: "task", "HTTP 请求发送失败: url={url_str}, error={e}");
        NodegetError::Other(format!("HTTP request failed: {e}"))
    })?;

    let status = resp.status().as_u16();

    let headers: Vec<BTreeMap<String, String>> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            let mut m = BTreeMap::new();
            let val = v.to_str().map_or_else(
                |_| BASE64_STANDARD.encode(v.as_bytes()),
                std::borrow::ToOwned::to_owned,
            );
            m.insert(k.as_str().to_owned(), val);
            m
        })
        .collect();

    let bytes = resp.bytes().await.map_err(|e| {
        warn!(target: "task", "HTTP 响应体读取失败: error={e}");
        NodegetError::Other(format!("Failed to read HTTP response body: {e}"))
    })?;
    let (body, body_base64) = String::from_utf8(bytes.to_vec()).map_or_else(
        |_| (None, Some(BASE64_STANDARD.encode(&bytes))),
        |s| (Some(s), None),
    );

    Ok(HttpRequestTaskResult {
        status,
        headers,
        body,
        body_base64,
    })
}
