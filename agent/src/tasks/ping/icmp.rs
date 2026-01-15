use parking_lot::Mutex;
use rand::random;
use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};
use tokio::sync::OnceCell;

static ICMP_PAYLOAD: [u8; 8] = [0; 8];
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
static GLOBAL_ICMP_V4_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();
static GLOBAL_ICMP_V6_CLIENT: OnceCell<Mutex<Client>> = OnceCell::const_new();

pub async fn ping_v4_target(target: std::net::IpAddr) -> Result<std::time::Duration, SurgeError> {
    let client_v4_mutex = GLOBAL_ICMP_V4_CLIENT
        .get_or_init(|| async {
            let config_v4 = Config::builder().kind(ICMP::V4).build();
            let client_v4 = Client::new(&config_v4).unwrap();
            Mutex::new(client_v4)
        })
        .await;

    let client = client_v4_mutex.lock();

    let mut pinger = client.pinger(target, PingIdentifier(random())).await;

    match pinger
        .timeout(PING_TIMEOUT)
        .ping(PingSequence(0), &ICMP_PAYLOAD)
        .await
    {
        Ok((_packet, duration)) => Ok(duration),
        Err(e) => Err(e),
    }
}

pub async fn ping_v6_target(target: std::net::IpAddr) -> Result<std::time::Duration, SurgeError> {
    let client_v6_mutex = GLOBAL_ICMP_V6_CLIENT
        .get_or_init(|| async {
            let config_v6 = Config::builder().kind(ICMP::V6).build();
            let client_v6 = Client::new(&config_v6).unwrap();
            Mutex::new(client_v6)
        })
        .await;

    let client = client_v6_mutex.lock();

    let mut pinger = client.pinger(target, PingIdentifier(random())).await;

    match pinger
        .timeout(PING_TIMEOUT)
        .ping(PingSequence(0), &ICMP_PAYLOAD)
        .await
    {
        Ok((_packet, duration)) => Ok(duration),
        Err(e) => Err(e),
    }
}
