//! DNS 查询任务模块。
//!
//! 使用 `hickory-resolver` 执行多种记录类型的 DNS 查询，
//! 支持自定义 DNS 服务器或回退到系统配置。

use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::proto::rr::{RData, RecordType};
use hickory_resolver::system_conf::read_system_conf;
use log::warn;
use ng_core::error::NodegetError;
use ng_task::{DnsRecordResult, DnsRecordType, DnsTask};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

/// DNS resolver 缓存，按"解析后的 DNS 服务器地址"复用 `TokioAsyncResolver`。
///
/// `TokioAsyncResolver` 内部维护 UDP 连接与查询缓存；原先每个 DNS 任务都新建一个
/// resolver 用完即弃，无法跨任务复用。这里按 `Option<SocketAddr>`（自定义服务器）/
/// `None`（系统配置）缓存，`TokioAsyncResolver` 是 `Clone`（内部 Arc），
/// 命中时仅一次原子计数自增。
///
/// **权衡**：`None`（系统配置）分支会缓存首次读取的 `/etc/resolv.conf`，之后
/// resolv.conf 的变更在进程重启前不生效。对常驻 agent 而言复用收益大于实时刷新。
/// 缓存上限 `RESOLVER_CACHE_MAX`，超限清空防止异常输入撑爆。
static RESOLVER_CACHE: LazyLock<Mutex<HashMap<Option<SocketAddr>, TokioAsyncResolver>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// resolver 缓存最大条目数。正常只有 1（系统）+ 几个自定义服务器。
const RESOLVER_CACHE_MAX: usize = 16;

/// 执行 DNS 查询任务。
///
/// - `task` - DNS 查询任务参数，包含域名、记录类型列表和可选的 DNS 服务器
///
/// 返回查询结果向量；所有记录类型查询均无结果时返回错误。
pub async fn query_dns(task: &DnsTask) -> Result<Vec<DnsRecordResult>, NodegetError> {
    let resolver = build_resolver(task.dns_server.as_deref()).await?;
    let mut results = Vec::new();

    for record_type in &task.record_types {
        let start = Instant::now();
        match query_single_type(&resolver, &task.domain, record_type).await {
            Ok(records) => {
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                for (rt, data) in records {
                    results.push(DnsRecordResult {
                        record_type: rt,
                        time: elapsed,
                        data,
                    });
                }
            }
            Err(e) => {
                warn!(
                    "DNS query failed for domain={}, record_type={:?}: {}",
                    task.domain, record_type, e
                );
            }
        }
    }

    if results.is_empty() && !task.record_types.is_empty() {
        return Err(NodegetError::Other(format!(
            "DNS query returned no results for domain '{}', all {} record types queried",
            task.domain,
            task.record_types.len()
        )));
    }

    Ok(results)
}

/// 构建 DNS 解析器（带缓存）。
///
/// - `dns_server` - 可选的自定义 DNS 服务器地址字符串
///
/// 指定服务器时解析为 `SocketAddr` 后按地址缓存；未指定时读取系统 DNS 配置后按
/// `None` 缓存。命中缓存仅 clone（Arc 自增），未命中才构建新 resolver。
#[allow(clippy::unused_async)]
async fn build_resolver(dns_server: Option<&str>) -> Result<TokioAsyncResolver, NodegetError> {
    // 先把字符串解析为确定的 cache key，避免 "1.1.1.1" 与 "1.1.1.1:53" 各存一份。
    let key: Option<SocketAddr> = match dns_server {
        Some(server_str) => Some(server_str.parse().map_err(|e| {
            NodegetError::Other(format!("Invalid DNS server address '{server_str}': {e}"))
        })?),
        None => None,
    };

    // 快速路径：命中缓存。
    if let Ok(cache) = RESOLVER_CACHE.lock()
        && let Some(resolver) = cache.get(&key)
    {
        return Ok(resolver.clone());
    }

    // 未命中：构建新 resolver。
    let resolver = if let Some(addr) = key {
        let mut config = ResolverConfig::new();
        config.add_name_server(NameServerConfig::new(addr, Protocol::Udp));
        TokioAsyncResolver::tokio(config, ResolverOpts::default())
    } else {
        let (config, opts) = read_system_conf()
            .map_err(|e| NodegetError::Other(format!("Failed to read system DNS config: {e}")))?;
        TokioAsyncResolver::tokio(config, opts)
    };

    // 写回缓存（超限清空，防异常输入撑爆）。
    if let Ok(mut cache) = RESOLVER_CACHE.lock() {
        if cache.len() >= RESOLVER_CACHE_MAX {
            cache.clear();
        }
        // 即使并发下另一线程已插入相同 key，clone 出来的 resolver 等价，覆盖无妨。
        cache.insert(key, resolver.clone());
    }

    Ok(resolver)
}

/// 查询单一记录类型的 DNS 记录。
///
/// - `resolver` - DNS 解析器
/// - `domain` - 查询域名
/// - `record_type` - 记录类型
///
/// 返回匹配的 `(记录类型, 数据字符串)` 向量；查询失败时返回错误。
async fn query_single_type(
    resolver: &TokioAsyncResolver,
    domain: &str,
    record_type: &DnsRecordType,
) -> Result<Vec<(DnsRecordType, String)>, NodegetError> {
    let mut results = Vec::new();
    match record_type {
        DnsRecordType::A => {
            let lookup = resolver
                .lookup_ip(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS A lookup failed: {e}")))?;
            for ip in lookup.iter().filter(std::net::IpAddr::is_ipv4) {
                results.push((DnsRecordType::A, ip.to_string()));
            }
        }
        DnsRecordType::Aaaa => {
            let lookup = resolver
                .lookup_ip(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS AAAA lookup failed: {e}")))?;
            for ip in lookup.iter().filter(std::net::IpAddr::is_ipv6) {
                results.push((DnsRecordType::Aaaa, ip.to_string()));
            }
        }
        DnsRecordType::Txt => {
            let lookup = resolver
                .txt_lookup(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS TXT lookup failed: {e}")))?;
            for txt in lookup.iter() {
                results.push((DnsRecordType::Txt, txt.to_string()));
            }
        }
        DnsRecordType::Ptr => {
            let ip: std::net::IpAddr = domain.parse().map_err(|e| {
                NodegetError::Other(format!(
                    "PTR record query requires a valid IP address as domain, got '{domain}': {e}"
                ))
            })?;
            let lookup = resolver
                .reverse_lookup(ip)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS PTR lookup failed: {e}")))?;
            for name in lookup.iter() {
                results.push((DnsRecordType::Ptr, name.to_string()));
            }
        }
        DnsRecordType::Mx => {
            let lookup = resolver
                .mx_lookup(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS MX lookup failed: {e}")))?;
            for mx in lookup.iter() {
                results.push((
                    DnsRecordType::Mx,
                    format!("{} {}", mx.preference(), mx.exchange()),
                ));
            }
        }
        DnsRecordType::Ns => {
            let lookup = resolver
                .ns_lookup(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS NS lookup failed: {e}")))?;
            for ns in lookup.iter() {
                results.push((DnsRecordType::Ns, ns.to_string()));
            }
        }
        DnsRecordType::Srv => {
            let lookup = resolver
                .srv_lookup(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS SRV lookup failed: {e}")))?;
            for srv in lookup.iter() {
                results.push((
                    DnsRecordType::Srv,
                    format!(
                        "{} {} {} {}",
                        srv.priority(),
                        srv.weight(),
                        srv.port(),
                        srv.target()
                    ),
                ));
            }
        }
        DnsRecordType::Soa => {
            let lookup = resolver
                .soa_lookup(domain)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS SOA lookup failed: {e}")))?;
            for soa in lookup.iter() {
                results.push((
                    DnsRecordType::Soa,
                    format!(
                        "{} {} {} {} {} {} {}",
                        soa.mname(),
                        soa.rname(),
                        soa.serial(),
                        soa.refresh(),
                        soa.retry(),
                        soa.expire(),
                        soa.minimum()
                    ),
                ));
            }
        }
        DnsRecordType::Cname => {
            let lookup = resolver
                .lookup(domain, RecordType::CNAME)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS CNAME lookup failed: {e}")))?;
            for record in lookup.record_iter() {
                if let Some(RData::CNAME(cname)) = record.data() {
                    results.push((DnsRecordType::Cname, cname.0.to_string()));
                }
            }
        }
        DnsRecordType::Caa => {
            let lookup = resolver
                .lookup(domain, RecordType::CAA)
                .await
                .map_err(|e| NodegetError::Other(format!("DNS CAA lookup failed: {e}")))?;
            for record in lookup.record_iter() {
                if let Some(RData::CAA(caa)) = record.data() {
                    results.push((
                        DnsRecordType::Caa,
                        format!("{} {:?}", caa.tag(), caa.value()),
                    ));
                }
            }
        }
    }
    Ok(results)
}
