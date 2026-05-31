use ng_core::error::NodegetError;
use reqwest::Client;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

// 全局 HTTP 客户端实例
static GLOBAL_CLIENT: OnceCell<Client> = OnceCell::const_new();
static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();
// HTTP Ping 超时时间，设定为 10 秒
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// HTTP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

fn ensure_rustls_ring_provider() {
    let () = RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// 对目标执行 HTTP Ping
//
// 该函数向指定的 URL 发送 HTTP GET 请求，并测量请求所需的时间
//
// # 参数
// * `target` - 目标 URL
//
// # 返回值
// 成功时返回请求耗时，失败时返回错误信息
pub async fn httping_target(target: url::Url) -> Result<std::time::Duration> {
    let client = GLOBAL_CLIENT
        .get_or_try_init(async || {
            ensure_rustls_ring_provider();
            Client::builder()
                .timeout(PING_TIMEOUT)
                .build()
                .map_err(|e| NodegetError::Other(format!("Failed to build HTTP ping client: {e}")))
        })
        .await?;

    let start = std::time::Instant::now();
    client
        .get(target)
        .send()
        .await
        .map(|_| start.elapsed())
        .map_err(|e| NodegetError::Other(format!("Failed to http ping target: {e}")))
}
