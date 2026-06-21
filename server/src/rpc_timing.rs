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
        // method_name/id 必须在 `service.call(request)`（move request）前提取，
        // 且 future 内仍需使用，故需 owned（受 jsonrpsee `Request<'a>` 借用约束）。
        let method_name = request.method_name().to_owned();
        let request_id = request.id().into_owned();
        let level = self.level;
        let service = self.service.clone();
        let started_at = Instant::now();

        async move {
            let response = service.call(request).await;
            let elapsed_us = started_at.elapsed().as_micros();
            // id 作为独立 tracing 字段内联，避免每请求 format! 分配 String；
            // 字段值仅在 level 启用时才格式化（tracing 惰性求值）。
            match level {
                Level::ERROR => tracing::error!(
                    target: "rpc", rpc_kind = "call", method = %method_name, elapsed_us, id = ?request_id, "rpc.call completed"
                ),
                Level::WARN => tracing::warn!(
                    target: "rpc", rpc_kind = "call", method = %method_name, elapsed_us, id = ?request_id, "rpc.call completed"
                ),
                Level::INFO => tracing::info!(
                    target: "rpc", rpc_kind = "call", method = %method_name, elapsed_us, id = ?request_id, "rpc.call completed"
                ),
                Level::DEBUG => tracing::debug!(
                    target: "rpc", rpc_kind = "call", method = %method_name, elapsed_us, id = ?request_id, "rpc.call completed"
                ),
                Level::TRACE => tracing::trace!(
                    target: "rpc", rpc_kind = "call", method = %method_name, elapsed_us, id = ?request_id, "rpc.call completed"
                ),
            }
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
        // 方法名列表仅在 level 启用时 join（延迟到 tracing 字段求值），避免未启用时分配。
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
            match level {
                Level::ERROR => tracing::error!(
                    target: "rpc", rpc_kind = "batch", methods = %methods, elapsed_us, size = batch_size, "rpc.batch completed"
                ),
                Level::WARN => tracing::warn!(
                    target: "rpc", rpc_kind = "batch", methods = %methods, elapsed_us, size = batch_size, "rpc.batch completed"
                ),
                Level::INFO => tracing::info!(
                    target: "rpc", rpc_kind = "batch", methods = %methods, elapsed_us, size = batch_size, "rpc.batch completed"
                ),
                Level::DEBUG => tracing::debug!(
                    target: "rpc", rpc_kind = "batch", methods = %methods, elapsed_us, size = batch_size, "rpc.batch completed"
                ),
                Level::TRACE => tracing::trace!(
                    target: "rpc", rpc_kind = "batch", methods = %methods, elapsed_us, size = batch_size, "rpc.batch completed"
                ),
            }
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
            match level {
                Level::ERROR => tracing::error!(
                    target: "rpc", rpc_kind = "notification", method = %method_name, elapsed_us, "rpc.notification completed"
                ),
                Level::WARN => tracing::warn!(
                    target: "rpc", rpc_kind = "notification", method = %method_name, elapsed_us, "rpc.notification completed"
                ),
                Level::INFO => tracing::info!(
                    target: "rpc", rpc_kind = "notification", method = %method_name, elapsed_us, "rpc.notification completed"
                ),
                Level::DEBUG => tracing::debug!(
                    target: "rpc", rpc_kind = "notification", method = %method_name, elapsed_us, "rpc.notification completed"
                ),
                Level::TRACE => tracing::trace!(
                    target: "rpc", rpc_kind = "notification", method = %method_name, elapsed_us, "rpc.notification completed"
                ),
            }
            response
        }
    }
}
