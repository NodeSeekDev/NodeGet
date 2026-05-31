use crate::AGENT_CONFIG;
use log::trace;
use ng_config::config::agent::IpProvider;
use reqwest::Client;
use reqwest::header::USER_AGENT;
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Once;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;

#[derive(Clone, Copy)]
enum IpFamily {
    Ipv4Only,
    Ipv6Only,
}

// reqwest client 构建在异步闭包里（见 `get_client`），必须用 `tokio::sync::OnceCell`
// 的 `get_or_try_init` async 版本；`std::sync::LazyLock` 的初始化闭包不能 await，
// 所以这两个保持 `OnceCell`。
static CLIENT_V4: OnceCell<Client> = OnceCell::const_new();
static CLIENT_V6: OnceCell<Client> = OnceCell::const_new();
// 单纯的 idempotent side effect 只需要 `Once`，没必要存结果。
static RUSTLS_PROVIDER_INIT: Once = Once::new();

#[derive(Debug)]
pub struct IPInfo {
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
}

pub async fn ip() -> IPInfo {
    let provider = AGENT_CONFIG
        .get()
        .and_then(|lock| {
            lock.read()
                .ok()
                .map(|config| config.ip_provider_or_default())
        })
        .unwrap_or_default();

    match provider {
        IpProvider::Cloudflare => ip_cloudflare().await,
        IpProvider::IpInfo => ip_ipinfo().await,
    }
}

fn ensure_rustls_ring_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn get_client(family: IpFamily) -> Option<&'static Client> {
    match family {
        IpFamily::Ipv4Only => CLIENT_V4
            .get_or_try_init(|| async {
                ensure_rustls_ring_provider();
                Client::builder()
                    .timeout(Duration::from_secs(5))
                    .local_address(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                    .build()
            })
            .await
            .map_err(|e| trace!("Failed to build IPv4 reqwest client: {e}"))
            .ok(),
        IpFamily::Ipv6Only => CLIENT_V6
            .get_or_try_init(|| async {
                ensure_rustls_ring_provider();
                Client::builder()
                    .timeout(Duration::from_secs(5))
                    .local_address(std::net::IpAddr::V6(Ipv6Addr::UNSPECIFIED))
                    .build()
            })
            .await
            .map_err(|e| trace!("Failed to build IPv6 reqwest client: {e}"))
            .ok(),
    }
}

// 通用 Get
async fn fetch_text(url: &str, family: IpFamily) -> Option<String> {
    let client = get_client(family).await?;
    client
        .get(url)
        .header(USER_AGENT, "curl/8.7.1")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()
}

// Parsers
fn parse_ipinfo_json(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    json.get("ip")?.as_str().map(ToOwned::to_owned)
}

fn parse_cloudflare_trace(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.starts_with("ip="))
        .map(|line| line.replace("ip=", ""))
}

// --- Providers ---

pub async fn ip_ipinfo() -> IPInfo {
    // IPv4 Task
    let ipv4: JoinHandle<Option<Ipv4Addr>> = tokio::spawn(async move {
        let body = fetch_text("https://ipinfo.io", IpFamily::Ipv4Only).await?;
        let ip_str = parse_ipinfo_json(&body)?;
        Ipv4Addr::from_str(&ip_str).ok()
    });

    // IPv6 Task
    let ipv6: JoinHandle<Option<Ipv6Addr>> = tokio::spawn(async move {
        let body = fetch_text("https://6.ipinfo.io", IpFamily::Ipv6Only).await?;
        let ip_str = parse_ipinfo_json(&body)?;
        Ipv6Addr::from_str(&ip_str).ok()
    });

    let ip_info = IPInfo {
        ipv4: ipv4.await.unwrap_or(None),
        ipv6: ipv6.await.unwrap_or(None),
    };

    trace!("IP (ipinfo) retrieved: {ip_info:?}");
    ip_info
}

pub async fn ip_cloudflare() -> IPInfo {
    // Use IP-literal URLs rather than `www.cloudflare.com` so DNS resolution
    // never picks a family that conflicts with `local_address`. The TLS
    // certificate served at 1.1.1.1 / 2606:4700:4700::1111 includes those
    // IPs in its SAN list, and `/cdn-cgi/trace` is available on both
    // anycast endpoints with the same `ip=<client-ip>` format.
    // IPv4 Task
    let ipv4: JoinHandle<Option<Ipv4Addr>> = tokio::spawn(async move {
        let body = fetch_text("https://1.1.1.1/cdn-cgi/trace", IpFamily::Ipv4Only).await?;
        let ip_str = parse_cloudflare_trace(&body)?;
        Ipv4Addr::from_str(&ip_str).ok()
    });

    // IPv6 Task
    let ipv6: JoinHandle<Option<Ipv6Addr>> = tokio::spawn(async move {
        let body = fetch_text(
            "https://[2606:4700:4700::1111]/cdn-cgi/trace",
            IpFamily::Ipv6Only,
        )
        .await?;
        let ip_str = parse_cloudflare_trace(&body)?;
        Ipv6Addr::from_str(&ip_str).ok()
    });

    let ip_info = IPInfo {
        ipv4: ipv4.await.unwrap_or(None),
        ipv6: ipv6.await.unwrap_or(None),
    };

    trace!("IP (cloudflare) retrieved: {ip_info:?}");
    ip_info
}
