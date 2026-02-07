use log::error;
use std::hint::black_box;
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;

// TCP 系统重传时间为 1 Sec 以上，请勿动本参数
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

// 对目标执行 TCP Ping
//
// 该函数尝试连接到目标主机的指定端口，并测量连接所需的时间
//
// # 参数
// * `target` - 目标地址（格式为 "host:port"）
//
// # 返回值
// 成功时返回连接耗时，失败时返回错误信息
pub async fn tcping_target(target: String) -> Result<std::time::Duration, String> {
    let target_host = lookup_host(target)
        .await
        .map_err(|e| error!("Resolving host error: {e}"))
        .ok()
        .and_then(|mut addrs| addrs.next())
        .ok_or("Invalid target")?;

    let start = std::time::Instant::now();
    timeout(PING_TIMEOUT, TcpStream::connect(target_host))
        .await
        .map_err(|_| "Tcp Ping Timeout".to_string())?
        .map_err(|_| "Tcp Ping Error".to_string())
        .map(|stream| {
            black_box(stream);
            start.elapsed()
        })
}
