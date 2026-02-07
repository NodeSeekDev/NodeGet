use tokio::sync::{Mutex, OnceCell};

// 全局 HTTP 客户端代理实例
static GLOBAL_AGENT: OnceCell<Mutex<ureq::Agent>> = OnceCell::const_new();
// HTTP Ping 超时时间，设定为 10 秒
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

// 对目标执行 HTTP Ping
//
// 该函数向指定的 URL 发送 HTTP GET 请求，并测量请求所需的时间
//
// # 参数
// * `target` - 目标 URL
//
// # 返回值
// 成功时返回请求耗时，失败时返回错误信息
pub async fn httping_target(target: url::Url) -> Result<std::time::Duration, String> {
    let agent = GLOBAL_AGENT
        .get_or_init(async || {
            Mutex::new(ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_global(Some(PING_TIMEOUT))
                    .build(),
            ))
        })
        .await;

    let start = std::time::Instant::now();
    agent
        .lock()
        .await
        .get(target.to_string())
        .call()
        .map(|_| start.elapsed())
        .map_err(|e| format!("Failed to http ping target: {e}"))
}
