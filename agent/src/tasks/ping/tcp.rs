use log::error;
use nodeget_lib::error::NodegetError;
use std::hint::black_box;
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;

// TCP 系统重传时间为 1 Sec 以上，请勿动本参数
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// TCP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// 对目标执行 TCP Ping
//
// 该函数尝试连接到目标主机的指定端口，并测量连接所需的时间
//
// # 参数
// * `target` - 目标地址（格式为 "host:port"）
//
// # 返回值
// 成功时返回连接耗时，失败时返回错误信息
pub async fn tcping_target(target: String) -> Result<std::time::Duration> {
    let target_host = lookup_host(target)
        .await
        .map_err(|e| {
            error!("Resolving host error: {e}");
            NodegetError::Other(format!("Resolving host error: {e}"))
        })?
        .next()
        .ok_or_else(|| NodegetError::Other("Invalid target".to_owned()))?;

    let start = std::time::Instant::now();
    timeout(PING_TIMEOUT, TcpStream::connect(target_host))
        .await
        .map_err(|_| NodegetError::Other("Tcp Ping Timeout".to_owned()))?
        .map_err(|_| NodegetError::Other("Tcp Ping Error".to_owned()))
        .map(|stream| {
            black_box(stream);
            start.elapsed()
        })
}
