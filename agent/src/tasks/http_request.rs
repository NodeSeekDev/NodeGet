use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use nodeget_lib::error::NodegetError;
use nodeget_lib::task::{HttpRequestTask, HttpRequestTaskResult};
use reqwest::{Client, Method};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;
use std::time::Duration;

pub type Result<T> = anyhow::Result<T>;

static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn ensure_rustls_ring_provider() {
    let () = RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub async fn execute_http_request(task: HttpRequestTask) -> Result<HttpRequestTaskResult> {
    ensure_rustls_ring_provider();

    let method = Method::from_bytes(task.method.trim().to_ascii_uppercase().as_bytes())
        .map_err(|e| NodegetError::InvalidInput(format!("Invalid http_request.method: {e}")))?;

    let mut builder = Client::builder().timeout(HTTP_REQUEST_TIMEOUT);
    if let Some(ip_raw) = task.ip.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let ip = match ip_raw.to_ascii_lowercase().as_str() {
            "ipv4 auto" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            "ipv6 auto" => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            _ => ip_raw.parse().map_err(|e| NodegetError::InvalidInput(format!("Invalid http_request.ip '{ip_raw}': {e}")))?,
        };
        builder = builder.local_address(ip);
    }
    let client = builder.build()
        .map_err(|e| NodegetError::Other(format!("Failed to build HTTP client: {e}")))?;

    let mut req = client.request(method, task.url);
    for (k, v) in &task.headers {
        req = req.header(k, v);
    }

    match (&task.body, &task.body_base64) {
        (Some(_), Some(_)) => {
            return Err(NodegetError::InvalidInput(
                "http_request.body and http_request.body_base64 are mutually exclusive".to_owned(),
            ).into());
        }
        (Some(body), None) => { req = req.body(body.clone()); }
        (None, Some(b64)) => {
            let bytes = BASE64_STANDARD.decode(b64)
                .map_err(|e| NodegetError::InvalidInput(format!("Invalid http_request.body_base64: {e}")))?;
            req = req.body(bytes);
        }
        (None, None) => {}
    }

    let resp = req.send().await
        .map_err(|e| NodegetError::Other(format!("HTTP request failed: {e}")))?;

    let status = resp.status().as_u16();

    let headers: Vec<BTreeMap<String, String>> = resp.headers().iter().map(|(k, v)| {
        let mut m = BTreeMap::new();
        let val = v.to_str().map_or_else(
            |_| BASE64_STANDARD.encode(v.as_bytes()),
            |s| s.to_owned(),
        );
        m.insert(k.as_str().to_owned(), val);
        m
    }).collect();

    let bytes = resp.bytes().await
        .map_err(|e| NodegetError::Other(format!("Failed to read HTTP response body: {e}")))?;
    let (body, body_base64) = match String::from_utf8(bytes.to_vec()) {
        Ok(s) => (Some(s), None),
        Err(_) => (None, Some(BASE64_STANDARD.encode(&bytes))),
    };

    Ok(HttpRequestTaskResult {
        status,
        headers,
        body,
        body_base64,
    })
}
