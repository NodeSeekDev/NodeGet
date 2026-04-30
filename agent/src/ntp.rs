use log::info;
use sntpc::{NtpContext, NtpTimestampGenerator, get_time};
use sntpc_net_tokio::UdpSocketWrapper;
use tokio::net::UdpSocket;
use tokio::time::{Duration, timeout};

const DEFAULT_NTP_PORT: u16 = 123;
const NTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Copy, Clone, Default)]
struct StdTimestampGen {
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
/// 连接失败或超时时返回 0，等同于使用本地时间。
pub async fn fetch_ntp_offset(ntp_server: &str) -> i64 {
    let Some(addr) = resolve_ntp_addr(ntp_server).await else {
        info!("Failed to resolve NTP server address for: {ntp_server}");
        return 0;
    };

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => UdpSocketWrapper::from(s),
        Err(e) => {
            info!("Failed to bind UDP socket for NTP: {e}");
            return 0;
        }
    };

    let context = NtpContext::new(StdTimestampGen::default());
    let result = timeout(NTP_TIMEOUT, get_time(addr, &socket, context)).await;

    match result {
        Ok(Ok(time)) => {
            let offset_us = time.offset();
            let offset_ms = offset_us / 1000;
            info!(
                "NTP sync success: server={ntp_server}, offset={offset_ms} ms (raw={offset_us} us)"
            );
            offset_ms
        }
        Ok(Err(e)) => {
            info!("NTP request failed for {ntp_server}: {e:?}");
            0
        }
        Err(_) => {
            info!("NTP request timed out after 10s for {ntp_server}");
            0
        }
    }
}

async fn resolve_ntp_addr(server: &str) -> Option<std::net::SocketAddr> {
    let with_port = format!("{server}:{DEFAULT_NTP_PORT}");
    let mut addrs = tokio::net::lookup_host(&with_port).await.ok()?;
    addrs.next()
}
