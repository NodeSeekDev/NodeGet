//! 停止信号监听
//!
//! 让 Agent 在收到"请你退出"的信号时先中止各个任务再退出，而不是被直接杀掉。
//! - Unix：`SIGTERM`（`docker stop`、`systemctl stop`、`kill`）与 `SIGINT`（终端 Ctrl+C）
//! - Windows：Ctrl+C
//!
//! 监听器在 `main` 里创建一次、在整个热重载循环中复用：每轮重新创建的话，
//! 两轮之间（重读配置的间隙）到达的信号会丢失。

#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};

/// 停止信号监听器
pub struct StopSignal {
    #[cfg(unix)]
    sigterm: Signal,
    #[cfg(unix)]
    sigint: Signal,
}

impl StopSignal {
    /// 注册信号处理
    ///
    /// 注册之后，进程不再按这些信号的默认动作立即终止；收到的信号会排队，
    /// 由 [`StopSignal::recv`] 取走。
    ///
    /// # Errors
    ///
    /// 系统拒绝注册信号处理时返回 I/O 错误。
    #[cfg(unix)]
    pub fn install() -> std::io::Result<Self> {
        Ok(Self {
            sigterm: signal(SignalKind::terminate())?,
            sigint: signal(SignalKind::interrupt())?,
        })
    }

    /// 注册信号处理（Windows 上 Ctrl+C 的处理在等待时才注册，这里无事可做）
    ///
    /// # Errors
    ///
    /// 与 Unix 版本签名保持一致，Windows 上不会返回错误。
    #[cfg(not(unix))]
    #[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)] // 签名需与 Unix 版一致
    pub fn install() -> std::io::Result<Self> {
        Ok(Self {})
    }

    /// 等待下一个停止信号，返回信号名称（用于日志）
    #[cfg(unix)]
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.sigterm.recv() => "SIGTERM",
            _ = self.sigint.recv() => "SIGINT",
        }
    }

    /// 等待下一个停止信号，返回信号名称（用于日志）
    #[cfg(not(unix))]
    #[allow(clippy::needless_pass_by_ref_mut)] // 签名需与 Unix 版一致
    pub async fn recv(&mut self) -> &'static str {
        // 注册失败时不能当作收到信号，否则 Agent 一启动就会退出：改为永远等待
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
        "Ctrl+C"
    }
}
