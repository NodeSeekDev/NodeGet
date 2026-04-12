use log::error;
#[cfg(not(target_os = "windows"))]
use log::{debug, warn};
use nodeget_lib::error::NodegetError;
use rand::random;
use std::net::IpAddr;
use std::time::Duration;
use tokio::net::lookup_host;

/// ICMP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// ICMP Ping 负载数据
static ICMP_PAYLOAD: [u8; 8] = [0; 8];
// Ping 超时时间，设定为 2 秒
static PING_TIMEOUT: Duration = Duration::from_secs(2);

// ─── Unprivileged ICMP socket (SOCK_DGRAM) ─────────────────────────────
// Linux 3.0+ 支持 SOCK_DGRAM + IPPROTO_ICMP，无需 root / CAP_NET_RAW
// 需要 sysctl net.ipv4.ping_group_range 包含当前用户 GID
// macOS 也支持此模式
#[cfg(not(target_os = "windows"))]
mod dgram {
    use super::*;
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::net::SocketAddr;
    use std::time::Instant;

    // ICMP Echo Request/Reply type codes
    const ICMP_ECHO_REQUEST: u8 = 8;
    const ICMP_ECHO_REPLY: u8 = 0;
    const ICMPV6_ECHO_REQUEST: u8 = 128;
    const ICMPV6_ECHO_REPLY: u8 = 129;

    /// 计算 ICMP 校验和（RFC 1071 one's complement）
    fn icmp_checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i + 1 < data.len() {
            sum += u32::from(u16::from_be_bytes([data[i], data[i + 1]]));
            i += 2;
        }
        if i < data.len() {
            sum += u32::from(data[i]) << 8;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !sum as u16
    }

    /// 构造 ICMP Echo Request 报文
    /// 格式: type(1) + code(1) + checksum(2) + id(2) + seq(2) + payload
    fn build_echo_request(is_v6: bool, id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
        let ty = if is_v6 {
            ICMPV6_ECHO_REQUEST
        } else {
            ICMP_ECHO_REQUEST
        };
        let len = 8 + payload.len();
        let mut buf = vec![0u8; len];
        buf[0] = ty;
        buf[1] = 0; // code
        // checksum 先置零
        buf[4..6].copy_from_slice(&id.to_be_bytes());
        buf[6..8].copy_from_slice(&seq.to_be_bytes());
        buf[8..].copy_from_slice(payload);

        // ICMPv6 校验和由内核计算（DGRAM socket），ICMPv4 需要自己算
        if !is_v6 {
            let cksum = icmp_checksum(&buf);
            buf[2..4].copy_from_slice(&cksum.to_be_bytes());
        }
        buf
    }

    /// 尝试创建 unprivileged ICMP socket
    /// 返回 Err 表示系统不支持或权限不足，应 fallback 到 surge_ping
    fn try_create_socket(is_v6: bool) -> std::io::Result<Socket> {
        let (domain, protocol) = if is_v6 {
            (Domain::IPV6, Protocol::ICMPV6)
        } else {
            (Domain::IPV4, Protocol::ICMPV4)
        };
        let socket = Socket::new(domain, Type::DGRAM, Some(protocol))?;
        socket.set_nonblocking(true)?;
        socket.set_read_timeout(Some(PING_TIMEOUT))?;
        socket.set_write_timeout(Some(PING_TIMEOUT))?;
        Ok(socket)
    }

    /// 使用 DGRAM ICMP socket 执行 ping
    /// 成功返回 RTT，失败返回 io::Error
    async fn dgram_ping(target: IpAddr) -> std::io::Result<Duration> {
        let is_v6 = target.is_ipv6();
        let socket = try_create_socket(is_v6)?;

        let id: u16 = random();
        let seq: u16 = 0;
        let packet = build_echo_request(is_v6, id, seq, &ICMP_PAYLOAD);

        let dest: SocketAddr = match target {
            IpAddr::V4(v4) => SocketAddr::new(IpAddr::V4(v4), 0),
            IpAddr::V6(v6) => SocketAddr::new(IpAddr::V6(v6), 0),
        };
        let dest_addr = SockAddr::from(dest);

        // 使用 spawn_blocking 因为 socket2 是同步 API
        let duration = tokio::task::spawn_blocking(move || -> std::io::Result<Duration> {
            // 设置阻塞超时（spawn_blocking 内部是同步的）
            socket.set_nonblocking(false)?;
            socket.set_read_timeout(Some(PING_TIMEOUT))?;

            let start = Instant::now();
            socket.send_to(&packet, &dest_addr)?;

            let mut recv_buf = [0u8; 256];
            let deadline = start + PING_TIMEOUT;

            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "ICMP ping timeout",
                    ));
                }
                socket.set_read_timeout(Some(remaining))?;

                let (n, _from) = match socket.recv_from(&mut recv_buf) {
                    Ok(r) => r,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "ICMP ping timeout",
                        ));
                    }
                    Err(e) => return Err(e),
                };

                if n < 8 {
                    continue;
                }

                // DGRAM socket 返回的数据不含 IP 头，直接是 ICMP 报文
                let reply_type = recv_buf[0];
                let expected_reply = if is_v6 {
                    ICMPV6_ECHO_REPLY
                } else {
                    ICMP_ECHO_REPLY
                };

                if reply_type != expected_reply {
                    continue;
                }

                // 对于 DGRAM ICMP socket，内核会按 id 做 demux
                // 但为安全起见仍然校验 id
                let reply_id = u16::from_be_bytes([recv_buf[4], recv_buf[5]]);
                if reply_id != id {
                    continue;
                }

                return Ok(start.elapsed());
            }
        })
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;

        Ok(duration)
    }

    /// 尝试使用 unprivileged ICMP socket ping
    /// 返回 Ok(Some(duration)) 成功，Ok(None) 表示不支持应 fallback，Err 表示 ping 失败
    pub async fn try_ping(target: IpAddr) -> std::result::Result<Option<Duration>, NodegetError> {
        match dgram_ping(target).await {
            Ok(d) => Ok(Some(d)),
            Err(e) => {
                // EPERM / EACCES / EPROTONOSUPPORT / EAFNOSUPPORT → 不支持，应 fallback
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        debug!("ICMP DGRAM socket permission denied, falling back to raw socket");
                        Ok(None)
                    }
                    _ if e.raw_os_error() == Some(libc::EPROTONOSUPPORT)
                        || e.raw_os_error() == Some(libc::EACCES) =>
                    {
                        debug!(
                            "ICMP DGRAM socket not supported (os error {}), falling back to raw socket",
                            e.raw_os_error().unwrap_or(0)
                        );
                        Ok(None)
                    }
                    std::io::ErrorKind::TimedOut => {
                        Err(NodegetError::Other("ICMP ping timeout".to_owned()))
                    }
                    _ => Err(NodegetError::Other(format!("ICMP ping error: {e}"))),
                }
            }
        }
    }
}

// ─── surge_ping fallback (raw socket, 需要 root / CAP_NET_RAW) ─────────
mod raw {
    use super::*;
    use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};
    use tokio::sync::{Mutex, OnceCell};

    static GLOBAL_ICMP_V4_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();
    static GLOBAL_ICMP_V6_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();

    async fn ping_v4_target(target: IpAddr) -> std::result::Result<Duration, SurgeError> {
        let client_mutex = GLOBAL_ICMP_V4_CLIENT
            .get_or_init(|| async {
                let config = Config::builder().kind(ICMP::V4).build();
                let client = Client::new(&config).unwrap();
                Mutex::new(client)
            })
            .await;

        let mut pinger = {
            let client = client_mutex.lock().await;
            client.pinger(target, PingIdentifier(random())).await
        };

        pinger
            .timeout(PING_TIMEOUT)
            .ping(PingSequence(0), &ICMP_PAYLOAD)
            .await
            .map(|(_packet, duration)| duration)
    }

    async fn ping_v6_target(target: IpAddr) -> std::result::Result<Duration, SurgeError> {
        let client_mutex = GLOBAL_ICMP_V6_CLIENT
            .get_or_init(|| async {
                let config = Config::builder().kind(ICMP::V6).build();
                let client = Client::new(&config).unwrap();
                Mutex::new(client)
            })
            .await;

        let mut pinger = {
            let client = client_mutex.lock().await;
            client.pinger(target, PingIdentifier(random())).await
        };

        pinger
            .timeout(PING_TIMEOUT)
            .ping(PingSequence(0), &ICMP_PAYLOAD)
            .await
            .map(|(_packet, duration)| duration)
    }

    pub async fn ping(target: IpAddr) -> Result<Duration> {
        if target.is_ipv4() {
            ping_v4_target(target)
                .await
                .map_err(|e| NodegetError::Other(format!("{e}")))
        } else {
            ping_v6_target(target)
                .await
                .map_err(|e| NodegetError::Other(format!("{e}")))
        }
    }
}

// ─── 解析目标地址 ───────────────────────────────────────────────────────
async fn resolve_target(target: &str) -> Result<IpAddr> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(ip);
    }

    match lookup_host(format!("{target}:80")).await {
        Ok(mut addrs) => addrs.next().map(|e| e.ip()).ok_or_else(|| {
            NodegetError::Other("DNS resolved but no addresses returned".to_owned())
        }),
        Err(e) => {
            error!("Resolving host error: {e}");
            Err(NodegetError::Other(format!(
                "Resolving host error: {e}"
            )))
        }
    }
}

// ─── 公开入口 ───────────────────────────────────────────────────────────

/// 对目标执行 ICMP Ping
///
/// 非 Windows 平台优先使用 unprivileged ICMP socket（SOCK_DGRAM），
/// 不需要 root 权限。如果系统不支持或权限不足，自动 fallback 到
/// surge_ping（raw socket，需要 root / CAP_NET_RAW）。
///
/// Windows 平台直接使用 surge_ping。
pub async fn ping_target(target: String) -> Result<Duration> {
    let ip = resolve_target(&target).await?;

    // 非 Windows：先尝试 unprivileged ICMP socket
    #[cfg(not(target_os = "windows"))]
    {
        match dgram::try_ping(ip).await {
            Ok(Some(duration)) => return Ok(duration),
            Ok(None) => {
                // 不支持，fallback
                warn!("Unprivileged ICMP socket unavailable for {target}, using raw socket fallback");
            }
            Err(e) => return Err(e),
        }
    }

    // Windows 或 fallback
    raw::ping(ip).await
}
