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
    let target_host = match lookup_host(target).await {
        Ok(mut addrs) => addrs.next(),
        Err(e) => {
            error!("Resolving host error: {e}");
            None
        }
    };

    let Some(target) = target_host else {
        return Err("Invalid target".to_string());
    };

    let start = std::time::Instant::now();
    match timeout(PING_TIMEOUT, TcpStream::connect(target)).await {
        Ok(Ok(stream)) => {
            black_box(stream);
            Ok(start.elapsed())
        }
        _ => Err("Http Ping Error".to_string()),
    }
}
