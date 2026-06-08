//! 服务器 Runtime 桥接 —— 将 future 调度到长生命周期的服务器 tokio Runtime 上执行。
//!
//! JS Worker 运行在专用的 current-thread Tokio Runtime 中，
//! 但涉及数据库连接池、RPC 等服务器资源的操作必须在服务器 Runtime 上执行，
//! 否则 runtime-bound 的 IO 资源会被错误地回收到不匹配的 executor。

use std::future::Future;
use std::sync::OnceLock;

/// 服务器 tokio Runtime 的 Handle，启动时通过 `init` 注入。
static SERVER_RUNTIME_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

/// 初始化服务器 Runtime Handle，必须在服务器启动时调用一次。
pub fn init(handle: tokio::runtime::Handle) {
    let _ = SERVER_RUNTIME_HANDLE.set(handle);
}

/// 将 future 提交到服务器 Runtime 上执行，返回结果。
///
/// JS Worker 运行在专用的短生命周期 / current-thread Tokio Runtime 中，
/// 涉及服务器服务或数据库连接池的调用必须在长生命周期的服务器 Runtime 上执行，
/// 否则 runtime-bound 的 IO 资源可能被回收到错误的 executor。
///
/// # Errors
///
/// 若服务器 Runtime Handle 未初始化，或 spawn 出的任务执行失败，返回错误字符串。
pub async fn spawn_on_server_runtime<F, T>(future: F) -> Result<T, String>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = SERVER_RUNTIME_HANDLE
        .get()
        .ok_or_else(|| "server runtime handle is not initialized".to_owned())?;

    // AbortOnDrop 确保若外层 future 被取消，spawn 出的 task 也会被终止
    let mut task = AbortOnDrop {
        handle: handle.spawn(future),
    };

    (&mut task.handle)
        .await
        .map_err(|e| format!("server runtime task failed: {e}"))
}

/// Drop 时自动 abort 尚未完成的 JoinHandle，防止泄露后台 task。
struct AbortOnDrop<T> {
    /// JoinHandle，drop 时自动 abort
    handle: tokio::task::JoinHandle<T>,
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if !self.handle.is_finished() {
            self.handle.abort();
        }
    }
}
