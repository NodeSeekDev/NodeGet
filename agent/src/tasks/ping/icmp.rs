use log::error;
use nodeget_lib::error::NodegetError;
use rand::random;
use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};
use tokio::net::lookup_host;
use tokio::sync::OnceCell;

/// ICMP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// ICMP Ping 负载数据
static ICMP_PAYLOAD: [u8; 8] = [0; 8];
// Ping 超时时间，设定为 2 秒
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

// 全局 IPv4 ICMP 客户端实例
// Client 内部使用 Arc 共享 socket 和 recv_task，Clone 即可并发使用，无需 Mutex
static GLOBAL_ICMP_V4_CLIENT: OnceCell<Client> = OnceCell::const_new();
// 全局 IPv6 ICMP 客户端实例
static GLOBAL_ICMP_V6_CLIENT: OnceCell<Client> = OnceCell::const_new();

async fn get_v4_client() -> &'static Client {
    GLOBAL_ICMP_V4_CLIENT
        .get_or_init(|| async {
            let config = Config::builder().kind(ICMP::V4).build();
            Client::new(&config).unwrap()
        })
        .await
}

async fn get_v6_client() -> &'static Client {
    GLOBAL_ICMP_V6_CLIENT
        .get_or_init(|| async {
            let config = Config::builder().kind(ICMP::V6).build();
            Client::new(&config).unwrap()
        })
        .await
}

// 对目标执行 ICMP Ping
//
// # 参数
// * `target` - 目标 IP 地址
//
// # 返回值
// 成功时返回往返时间，失败时返回错误
async fn ping_ip(target: std::net::IpAddr) -> std::result::Result<std::time::Duration, SurgeError> {
    let client = if target.is_ipv4() {
        get_v4_client().await
    } else {
        get_v6_client().await
    };

    let mut pinger = client.pinger(target, PingIdentifier(random())).await;
    pinger.timeout(PING_TIMEOUT);

    let (_, duration) = pinger.ping(PingSequence(random()), &ICMP_PAYLOAD).await?;

    Ok(duration)
}

// 对目标执行 ICMP Ping
//
// 该函数首先尝试解析目标地址（如果是域名则进行 DNS 查询），然后根据 IP 版本选择合适的协议进行 Ping
//
// # 参数
// * `target` - 目标地址（可以是 IP 或域名）
//
// # 返回值
// 成功时返回往返时间，失败时返回错误信息
pub async fn ping_target(target: String) -> Result<std::time::Duration> {
    let target_ip = match target.parse::<std::net::IpAddr>() {
        Ok(ip) => Some(ip),
        Err(_) => match lookup_host(format!("{}:{}", target, 80)).await {
            Ok(mut addrs) => addrs.next().map(|e| e.ip()),
            Err(e) => {
                error!("Resolving host error: {e}");
                None
            }
        },
    };

    let Some(target) = target_ip else {
        return Err(NodegetError::Other("Invalid target".to_owned()));
    };

    ping_ip(target)
        .await
        .map_err(|e| NodegetError::Other(format!("{e}")))
}
