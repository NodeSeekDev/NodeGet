use log::error;
use nodeget_lib::error::NodegetError;
use rand::random;
use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};
use tokio::net::lookup_host;
use tokio::sync::{Mutex, OnceCell};

/// ICMP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// ICMP Ping 负载数据
static ICMP_PAYLOAD: [u8; 8] = [0; 8];
// Ping 超时时间，设定为 2 秒
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
// 全局 IPv4 ICMP 客户端实例
static GLOBAL_ICMP_V4_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();
// 全局 IPv6 ICMP 客户端实例
static GLOBAL_ICMP_V6_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();

// 对 IPv4 目标执行 ICMP Ping
//
// # 参数
// * `target` - 目标 IP 地址
//
// # 返回值
// 成功时返回往返时间，失败时返回错误
async fn ping_v4_target(target: std::net::IpAddr) -> std::result::Result<std::time::Duration, SurgeError> {
    let client_v4_mutex = GLOBAL_ICMP_V4_CLIENT
        .get_or_init(|| async {
            let config_v4 = Config::builder().kind(ICMP::V4).build();
            let client_v4 = Client::new(&config_v4).unwrap();
            Mutex::new(client_v4)
        })
        .await;

    let mut pinger = {
        let client = client_v4_mutex.lock().await;
        client.pinger(target, PingIdentifier(random())).await
    };

    match pinger
        .timeout(PING_TIMEOUT)
        .ping(PingSequence(0), &ICMP_PAYLOAD)
        .await
    {
        Ok((_packet, duration)) => Ok(duration),
        Err(e) => Err(e),
    }
}

// 对 IPv6 目标执行 ICMP Ping
//
// # 参数
// * `target` - 目标 IP 地址
//
// # 返回值
// 成功时返回往返时间，失败时返回错误
async fn ping_v6_target(target: std::net::IpAddr) -> std::result::Result<std::time::Duration, SurgeError> {
    let client_v6_mutex = GLOBAL_ICMP_V6_CLIENT
        .get_or_init(|| async {
            let config_v6 = Config::builder().kind(ICMP::V6).build();
            let client_v6 = Client::new(&config_v6).unwrap();
            Mutex::new(client_v6)
        })
        .await;

    let mut pinger = {
        let client = client_v6_mutex.lock().await;
        client.pinger(target, PingIdentifier(random())).await
    };

    match pinger
        .timeout(PING_TIMEOUT)
        .ping(PingSequence(0), &ICMP_PAYLOAD)
        .await
    {
        Ok((_packet, duration)) => Ok(duration),
        Err(e) => Err(e),
    }
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

    if target.is_ipv4() {
        ping_v4_target(target).await.map_err(|e| NodegetError::Other(format!("{e}")))
    } else {
        ping_v6_target(target).await.map_err(|e| NodegetError::Other(format!("{e}")))
    }
}
