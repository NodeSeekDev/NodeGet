use crate::AGENT_CONFIG;
use log::trace;
use nodeget_lib::config::agent::IpProvider;
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::time::Duration;
use tokio::task::JoinHandle;
use ureq::config::IpFamily;

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
                .and_then(|config| config.ip_provider.clone())
        })
        .unwrap_or(IpProvider::Cloudflare);

    match provider {
        IpProvider::Cloudflare => ip_cloudflare().await,
        IpProvider::IpInfo => ip_ipinfo().await,
    }
}

// 通用 Get
fn fetch_text(url: &str, family: IpFamily) -> Option<String> {
    ureq::get(url)
        .header("User-Agent", "curl/8.7.1")
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .ip_family(family)
        .build()
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()
}

// Parsers
fn parse_ipinfo_json(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    let ip = json.get("ip")?;
    Some(ip.to_string())
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
        let body = fetch_text("https://ipinfo.io", IpFamily::Ipv4Only)?;
        let ip_str = parse_ipinfo_json(&body)?;
        Ipv4Addr::from_str(&ip_str).ok()
    });

    // IPv6 Task
    let ipv6: JoinHandle<Option<Ipv6Addr>> = tokio::spawn(async move {
        let body = fetch_text("https://6.ipinfo.io", IpFamily::Ipv6Only)?;
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
    // IPv4 Task
    let ipv4: JoinHandle<Option<Ipv4Addr>> = tokio::spawn(async move {
        let body = fetch_text(
            "https://www.cloudflare.com/cdn-cgi/trace",
            IpFamily::Ipv4Only,
        )?;
        let ip_str = parse_cloudflare_trace(&body)?;
        Ipv4Addr::from_str(&ip_str).ok()
    });

    // IPv6 Task
    let ipv6: JoinHandle<Option<Ipv6Addr>> = tokio::spawn(async move {
        let body = fetch_text(
            "https://www.cloudflare.com/cdn-cgi/trace",
            IpFamily::Ipv6Only,
        )?;
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
