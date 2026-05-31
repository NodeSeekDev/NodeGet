use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::proto::rr::{RData, RecordType};
use hickory_resolver::system_conf::read_system_conf;
use log::warn;
use ng_core::error::NodegetError;
use ng_task::{DnsRecordResult, DnsRecordType, DnsTask};
use std::net::SocketAddr;
use std::time::Instant;

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

async fn build_resolver(dns_server: Option<&str>) -> Result<TokioAsyncResolver, NodegetError> {
    if let Some(server_str) = dns_server {
        let addr: SocketAddr = server_str.parse().map_err(|e| {
            NodegetError::Other(format!("Invalid DNS server address '{server_str}': {e}"))
        })?;
        let mut config = ResolverConfig::new();
        config.add_name_server(NameServerConfig::new(addr, Protocol::Udp));
        Ok(TokioAsyncResolver::tokio(config, ResolverOpts::default()))
    } else {
        let (config, opts) = read_system_conf()
            .map_err(|e| NodegetError::Other(format!("Failed to read system DNS config: {e}")))?;
        Ok(TokioAsyncResolver::tokio(config, opts))
    }
}

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
