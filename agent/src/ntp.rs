//! NTP 时间偏移获取模块。
//!
//! 启动时向配置的 NTP 服务器查询本地时间与参考时间的偏差，
//! 校准结果通过 [`ng_core::utils::set_ntp_offset_ms`] 写入全局偏移，
//! 供后续时间戳生成使用。连接失败或超时时回退到本地时间（偏移为 0）。

use futures_util::stream::{FuturesUnordered, StreamExt};
use log::{info, warn};
use sntpc::{NtpContext, NtpTimestampGenerator, get_time};
use sntpc_net_tokio::UdpSocketWrapper;
use tokio::net::UdpSocket;
use tokio::time::{Duration, timeout};

/// NTP 协议默认端口。
const DEFAULT_NTP_PORT: u16 = 123;
/// 单次 NTP 请求超时时间。
const NTP_TIMEOUT: Duration = Duration::from_secs(10);

/// 基于 `SystemTime` 的 NTP 时间戳生成器，供 sntpc 库使用。
#[derive(Copy, Clone, Default)]
struct StdTimestampGen {
    /// 距 Unix Epoch 的时长，`init()` 时设置。
    duration: Option<std::time::Duration>,
}

impl NtpTimestampGenerator for StdTimestampGen {
    fn init(&mut self) {
        self.duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok();
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.map_or(0, |d| d.as_secs())
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.duration.map_or(0, |d| d.subsec_micros())
    }
}

/// 从指定的 NTP 服务器获取本地时间与 NTP 参考时间的偏差（毫秒）。
///
/// - `ntp_server` - NTP 服务器主机名或 IP 地址
///
/// 连接失败或超时时返回 0，等同于使用本地时间。
pub async fn fetch_ntp_offset(ntp_server: &str) -> i64 {
    let addrs = match resolve_ntp_addrs(ntp_server).await {
        Some(a) if !a.is_empty() => a,
        _ => {
            warn!(
                "Failed to resolve NTP server address for: {ntp_server}; falling back to local time (offset=0)"
            );
            return 0;
        }
    };

    // 并发探测全部已解析地址：哪个地址族（IPv4/IPv6）先成功就直接采用，
    // 避免串行重试在多地址全部超时时把启动时间放大到 N * timeout。
    let mut probes = FuturesUnordered::new();
    for addr in addrs {
        probes.push(probe_ntp_addr(addr));
    }

    while let Some(result) = probes.next().await {
        match result {
            Ok((addr, offset_us)) => {
                // 有符号整数除法在 Rust 中对负数向 0 截断，例如 -1999 / 1000 = -1 而非 -2。
                // NTP offset 在局域网里经常就是 ±几十 us 这种小数量级，直接整除会把符号丢掉并且
                // 偏离真实值。用 f64 round 做四舍五入到最近的 ms，再转回 i64。
                #[allow(clippy::cast_possible_truncation)]
                let offset_ms = (offset_us as f64 / 1000.0).round() as i64;
                info!(
                    "NTP sync success: server={ntp_server}, addr={addr}, offset={offset_ms} ms (raw={offset_us} us)"
                );
                return offset_ms;
            }
            Err((addr, msg)) => {
                warn!("NTP probe failed for {ntp_server} ({addr}): {msg}; waiting for other addresses");
            }
        }
    }

    warn!(
        "All resolved NTP addresses for {ntp_server} failed; falling back to local time (offset=0)"
    );
    0
}

/// 对单个已解析的 NTP 地址发起一次探测。
///
/// 返回成功时的 `(addr, 原始微秒偏移)`；失败时返回 `(addr, 错误消息)` 供调用方统一记录。
async fn probe_ntp_addr(
    addr: std::net::SocketAddr,
) -> Result<(std::net::SocketAddr, i64), (std::net::SocketAddr, String)> {
    let bind_addr = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let socket = UdpSocket::bind(bind_addr).await.map_err(|e| {
        (
            addr,
            format!("failed to bind {bind_addr} for address family: {e}"),
        )
    })?;

    let context = NtpContext::new(StdTimestampGen::default());
    match timeout(NTP_TIMEOUT, get_time(addr, &UdpSocketWrapper::from(socket), context)).await {
        Ok(Ok(time)) => Ok((addr, time.offset())),
        Ok(Err(e)) => Err((addr, format!("request error: {e:?}"))),
        Err(_) => Err((addr, format!("timed out after {}s", NTP_TIMEOUT.as_secs()))),
    }
}

/// 将 NTP 服务器地址解析为全部 `SocketAddr`（默认端口 123）。
///
/// - `server` - NTP 服务器主机名或 IP 地址
///
/// 返回所有解析结果（通常含 IPv4 与/或 IPv6）；解析失败返回 `None`。
/// 保留全部地址以便按地址族逐一尝试，兼容纯 IPv4 / 纯 IPv6 环境。
async fn resolve_ntp_addrs(server: &str) -> Option<Vec<std::net::SocketAddr>> {
    let with_port = format!("{server}:{DEFAULT_NTP_PORT}");
    let addrs = tokio::net::lookup_host(&with_port).await.ok()?;
    Some(addrs.collect())
}
