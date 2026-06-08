//! RPC 请求计时中间件
//!
//! 横切关注点：测量每个 RPC 调用/批量请求/通知的耗时，
//! 以可配置的 tracing 级别输出到 `target: "rpc"`。
//! 所有 tracing 输出统一使用 `target: "rpc"` 而非模块级 target，
//! 因为该中间件是框架级横切关注点，与具体业务模块无关。

use jsonrpsee::server::middleware::rpc::{Batch, Notification, Request, RpcServiceT};
use std::future::Future;
use std::time::Instant;
use tracing::Level;

/// RPC 计时中间件
///
/// 包裹内部 RPC 服务，在每个请求完成时记录耗时（微秒）。
///
/// - service：被包裹的内部 RPC 服务
/// - level：tracing 输出级别，由 serve 启动时配置
#[derive(Clone)]
pub struct RpcTimingMiddleware<S> {
    /// 被包裹的内部 RPC 服务
    pub service: S,
    /// tracing 输出级别
    pub level: Level,
}

/// 按指定 tracing 级别输出 RPC 耗时日志
///
/// - level：输出级别
/// - method：RPC 方法名
/// - kind：请求类型（"call" / "batch" / "notification"）
/// - `elapsed_us`：耗时（微秒）
/// - extra：附加信息（请求 ID、批量大小等）
fn log_with_level(level: Level, method: &str, kind: &str, elapsed_us: u128, extra: &str) {
    match level {
        Level::ERROR => {
            tracing::error!(target: "rpc", rpc_kind = kind, method = method, elapsed_us = elapsed_us, "{extra}");
        }
        Level::WARN => {
            tracing::warn!(target: "rpc", rpc_kind = kind, method = method, elapsed_us = elapsed_us, "{extra}");
        }
        Level::INFO => {
            tracing::info!(target: "rpc", rpc_kind = kind, method = method, elapsed_us = elapsed_us, "{extra}");
        }
        Level::DEBUG => {
            tracing::debug!(target: "rpc", rpc_kind = kind, method = method, elapsed_us = elapsed_us, "{extra}");
        }
        Level::TRACE => {
            tracing::trace!(target: "rpc", rpc_kind = kind, method = method, elapsed_us = elapsed_us, "{extra}");
        }
    }
}

impl<S> RpcServiceT for RpcTimingMiddleware<S>
where
    S: RpcServiceT + Send + Sync + Clone + 'static,
{
    type MethodResponse = S::MethodResponse;
    type NotificationResponse = S::NotificationResponse;
    type BatchResponse = S::BatchResponse;

    /// 处理单个 RPC 调用，记录方法名、请求 ID 和耗时
    fn call<'a>(
        &self,
        request: Request<'a>,
    ) -> impl Future<Output = Self::MethodResponse> + Send + 'a {
        let method_name = request.method_name().to_owned();
        let request_id = request.id().into_owned();
        let level = self.level;
        let service = self.service.clone();
        let started_at = Instant::now();

        async move {
            let response = service.call(request).await;
            let elapsed_us = started_at.elapsed().as_micros();
            log_with_level(
                level,
                &method_name,
                "call",
                elapsed_us,
                &format!("rpc.call completed id={request_id:?}"),
            );
            response
        }
    }

    /// 处理批量 RPC 请求，记录所有方法名、批量大小和耗时
    fn batch<'a>(&self, batch: Batch<'a>) -> impl Future<Output = Self::BatchResponse> + Send + 'a {
        let batch_size = batch.len();
        let mut method_names = Vec::with_capacity(batch_size);
        for entry in batch.iter() {
            match entry {
                Ok(item) => method_names.push(item.method_name().to_owned()),
                Err(_) => method_names.push("<invalid>".to_owned()),
            }
        }
        let methods = if method_names.is_empty() {
            "<empty>".to_owned()
        } else {
            method_names.join(",")
        };

        let level = self.level;
        let service = self.service.clone();
        let started_at = Instant::now();

        async move {
            let response = service.batch(batch).await;
            let elapsed_us = started_at.elapsed().as_micros();
            log_with_level(
                level,
                &methods,
                "batch",
                elapsed_us,
                &format!("rpc.batch completed size={batch_size}"),
            );
            response
        }
    }

    /// 处理 RPC 通知（无响应的调用），记录方法名和耗时
    fn notification<'a>(
        &self,
        n: Notification<'a>,
    ) -> impl Future<Output = Self::NotificationResponse> + Send + 'a {
        let method_name = n.method_name().to_owned();
        let level = self.level;
        let service = self.service.clone();
        let started_at = Instant::now();

        async move {
            let response = service.notification(n).await;
            let elapsed_us = started_at.elapsed().as_micros();
            log_with_level(
                level,
                &method_name,
                "notification",
                elapsed_us,
                "rpc.notification completed",
            );
            response
        }
    }
}
