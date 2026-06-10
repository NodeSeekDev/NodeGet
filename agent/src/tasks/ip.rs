//! IP 地址获取任务模块。
//!
//! 通过外部 API（Cloudflare / ipinfo.io）查询 Agent 的公网 IPv4 和 IPv6 地址。
//! 两种 Provider 各自使用 IP 地址字面量 URL 确保 DNS 解析与出站 IP 族一致。

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

/// IP 地址族枚举，用于选择出站 IP 版本。
#[derive(Clone, Copy)]
enum IpFamily {
    /// 仅 IPv4
    Ipv4Only,
    /// 仅 IPv6
    Ipv6Only,
}

// reqwest client 构建在异步闭包里（见 `get_client`），必须用 `tokio::sync::OnceCell`
// 的 `get_or_try_init` async 版本；`std::sync::LazyLock` 的初始化闭包不能 await，
// 所以这两个保持 `OnceCell`。
/// IPv4 reqwest 客户端单例。
static CLIENT_V4: OnceCell<Client> = OnceCell::const_new();
/// IPv6 reqwest 客户端单例。
static CLIENT_V6: OnceCell<Client> = OnceCell::const_new();
/// rustls crypto provider 初始化标记（幂等 side effect，用 `Once` 即可）。
static RUSTLS_PROVIDER_INIT: Once = Once::new();

/// IP 地址查询结果。
#[derive(Debug)]
pub struct IPInfo {
    /// 公网 IPv4 地址
    pub ipv4: Option<Ipv4Addr>,
    /// 公网 IPv6 地址
    pub ipv6: Option<Ipv6Addr>,
}

/// 根据 Agent 配置的 IP Provider 获取公网 IP 地址。
///
/// 返回 [`IPInfo`]，包含 IPv4 和 IPv6 地址（可能为 `None`）。
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

/// 确保 rustls aws-lc-rs crypto provider 已安装（幂等）。
fn ensure_rustls_aws_lc_rs_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// 获取指定 IP 族的 reqwest 客户端单例。
///
/// - `family` - IP 地址族（IPv4/IPv6）
///
/// 客户端绑定到对应的 UNSPECIFIED 地址，确保出站流量使用正确的 IP 族。
/// 返回客户端静态引用；构建失败时返回 `None`。
async fn get_client(family: IpFamily) -> Option<&'static Client> {
    match family {
        IpFamily::Ipv4Only => CLIENT_V4
            .get_or_try_init(|| async {
                ensure_rustls_aws_lc_rs_provider();
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
                ensure_rustls_aws_lc_rs_provider();
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

/// 通用 HTTP GET 请求，返回响应文本。
///
/// - `url` - 请求 URL
/// - `family` - 出站 IP 族
///
/// 返回响应文本；请求或解析失败时返回 `None`。
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

/// 解析 ipinfo.io 返回的 JSON，提取 `ip` 字段。
fn parse_ipinfo_json(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    json.get("ip")?.as_str().map(ToOwned::to_owned)
}

/// 解析 Cloudflare `/cdn-cgi/trace` 返回的文本，提取 `ip=` 行的值。
fn parse_cloudflare_trace(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.starts_with("ip="))
        .map(|line| line.replace("ip=", ""))
}

/// --- IP 提供商实现 ---
/// 通过 ipinfo.io 获取公网 IP 地址。
///
/// 并发查询 IPv4（`ipinfo.io`）和 IPv6（`6.ipinfo.io`）。
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

/// 通过 Cloudflare `/cdn-cgi/trace` 获取公网 IP 地址。
///
/// 使用 IP 地址字面量 URL（而非 `www.cloudflare.com`），确保 DNS 解析不会选择与
/// `local_address` 冲突的 IP 族。1.1.1.1 / `2606:4700:4700::1111` 的 TLS 证书
/// 包含这些 IP 的 SAN，`/cdn-cgi/trace` 在两个任播端点上均可用。
pub async fn ip_cloudflare() -> IPInfo {
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
