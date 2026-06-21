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
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;
use std::time::Duration;

/// HTTP 请求结果类型
pub type Result<T> = anyhow::Result<T>;

/// rustls crypto provider 初始化标记，防止重复安装。
static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();
/// 默认 reqwest Client 缓存（无 IP 绑定），避免每次请求重建连接池/TLS/DNS 缓存。
static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();
/// 按 `local_address` 缓存的 reqwest Client 池，避免自定义 IP 时每次重建。
static IP_BOUND_CLIENTS: std::sync::LazyLock<std::sync::Mutex<HashMap<IpAddr, Client>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
/// IP-bound Client 缓存最大条目数。
/// 正常场景只有 2~3 个（IPv4/IPv6 UNSPECIFIED + 偶尔一个字面 IP）；
/// 超过上限时清空整个缓存重建，防止异常场景下无限增长。
const IP_BOUND_CLIENTS_MAX_CAPACITY: usize = 32;
/// HTTP 请求超时时间，30 秒。
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// HTTP 响应体最大捕获字节数（64 MiB）。
///
/// reqwest 0.13 的 `ClientBuilder` 没有 `max_response_body_size`，`bytes()` 会把
/// 整个响应体一次性读进内存。这里用 `chunk()` 流式累计，超过上限立即停止并报错，
/// 防止恶意/异常对端返回超大响应导致 agent OOM。`HTTP_REQUEST_TIMEOUT` 只限时长不限体积。
/// （与 `execute.rs` 的 `max_capture` 同类防御。）
const HTTP_RESPONSE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 确保 rustls aws-lc-rs crypto provider 已安装（幂等）。
fn ensure_rustls_aws_lc_rs_provider() {
    let () = RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
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
    ensure_rustls_aws_lc_rs_provider();

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
        // 按 IP 缓存 Client：首次构建后复用，避免每次请求重建连接池/TLS/DNS 缓存。
        let mut cache = IP_BOUND_CLIENTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.get(&ip) {
            cached.clone()
        } else {
            // 缓存条目超过上限时清空，防止异常场景下无限增长。
            // 正常场景只有 2~3 个 IP，上限 32 远超正常用量。
            if cache.len() >= IP_BOUND_CLIENTS_MAX_CAPACITY {
                cache.clear();
            }
            let client = Client::builder()
                .timeout(HTTP_REQUEST_TIMEOUT)
                .local_address(ip)
                .build()
                .map_err(|e| {
                    warn!(target: "task", "HTTP 客户端构建失败: error={e}");
                    NodegetError::Other(format!("Failed to build HTTP client: {e}"))
                })?;
            cache.insert(ip, client.clone());
            client
        }
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

    let mut resp = req.send().await.map_err(|e| {
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

    // 流式读取响应体，超 HTTP_RESPONSE_MAX_BYTES 即报错，避免对端返回超大响应 OOM。
    // 用 `chunk()` 而非 `bytes()`：后者无界缓冲，前者可逐块累计并在超限时中断。
    let limit = HTTP_RESPONSE_MAX_BYTES;
    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let new_len = bytes.len().saturating_add(chunk.len());
                if new_len > limit {
                    warn!(
                        target: "task",
                        "HTTP 响应体超过上限: url={url_str}, limit={limit} bytes, got>={new_len}",
                    );
                    return Err(NodegetError::Other(format!(
                        "HTTP response body too large: exceeds {limit} bytes"
                    ))
                    .into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                warn!(target: "task", "HTTP 响应体读取失败: error={e}");
                return Err(
                    NodegetError::Other(format!("Failed to read HTTP response body: {e}")).into(),
                );
            }
        }
    }
    let (body, body_base64) = String::from_utf8(bytes.clone()).map_or_else(
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
