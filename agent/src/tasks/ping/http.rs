//! HTTP Ping 任务实现。
//!
//! 向目标 URL 发送 HTTP GET 请求，测量请求往返耗时。

use log::warn;
use ng_core::error::NodegetError;
use reqwest::Client;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

/// 全局 HTTP 客户端单例。
static GLOBAL_CLIENT: OnceCell<Client> = OnceCell::const_new();
/// rustls crypto provider 初始化标记。
static RUSTLS_PROVIDER_INIT: OnceLock<()> = OnceLock::new();
/// HTTP Ping 超时时间，10 秒。
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// HTTP Ping 结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

/// 确保 rustls aws-lc-rs crypto provider 已安装（幂等）。
fn ensure_rustls_aws_lc_rs_provider() {
    let () = RUSTLS_PROVIDER_INIT.get_or_init(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// 对目标执行 HTTP Ping。
///
/// - `target` - 目标 URL
///
/// 1. 初始化全局 HTTP 客户端（含 rustls provider）
/// 2. 发送 GET 请求并测量耗时
///
/// 成功时返回请求耗时；请求失败时返回错误。
pub async fn httping_target(target: url::Url) -> Result<std::time::Duration> {
    let client = GLOBAL_CLIENT
        .get_or_try_init(async || {
            ensure_rustls_aws_lc_rs_provider();
            Client::builder()
                .timeout(PING_TIMEOUT)
                .build()
                .map_err(|e| {
                    warn!(target: "task", "HTTP Ping 客户端构建失败: error={e}");
                    NodegetError::Other(format!("Failed to build HTTP ping client: {e}"))
                })
        })
        .await?;

    let start = std::time::Instant::now();
    client
        .get(target.clone())
        .send()
        .await
        .map(|_| start.elapsed())
        .map_err(|e| {
            warn!(target: "task", "HTTP Ping 请求失败: url={target}, error={e}");
            NodegetError::Other(format!("Failed to http ping target: {e}"))
        })
}
