//! 通用工具函数集合
//!
//! 提供错误 JSON 构造、时间戳（含 NTP 偏移）、随机字符串生成等基础能力。
//! `for-server` feature 下额外导出 `error_message` 与 `server_json` 子模块。

use crate::error::{NodegetError, Result};
use rand::distr::Alphanumeric;
use rand::{RngExt, rng};
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};

#[cfg(feature = "for-server")]
pub mod error_message;

pub mod version;

pub mod uuid;

#[cfg(feature = "for-server")]
pub mod server_json;

/// RPC 错误响应结构体，所有 RPC 方法统一返回此格式。
#[derive(Serialize, Deserialize)]
pub struct JsonError {
    /// 数值错误码，对应 `NodegetError::error_code()`
    pub error_id: i128,
    /// 人类可读的错误描述
    pub error_message: String,
}

/// NTP 偏移量（毫秒），用于校正本地时钟与服务器时钟的差值。
static NTP_OFFSET_MS: AtomicI64 = AtomicI64::new(0);

/// 设置 NTP 偏移量，Agent 在收到 Server 时间后调用。
///
/// - `offset_ms`：Server 时间与本地时间的差值（毫秒）
pub fn set_ntp_offset_ms(offset_ms: i64) {
    NTP_OFFSET_MS.store(offset_ms, Ordering::Relaxed);
}

/// 获取经 NTP 校正后的本地时间戳（毫秒）。
///
/// 1. 读取系统单调时间
/// 2. 叠加 NTP 偏移
/// 3. 校验无下溢后转为 u64
pub fn get_local_timestamp_ms() -> Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| NodegetError::Other(format!("System time error: {e}")))?;
    let offset = NTP_OFFSET_MS.load(Ordering::Relaxed);
    let millis = i64::try_from(duration.as_millis())
        .map_err(|e| NodegetError::Other(format!("Timestamp conversion error: {e}")))?
        .saturating_add(offset);
    if millis < 0 {
        return Err(NodegetError::Other("Timestamp underflow after NTP offset".to_owned()).into());
    }
    u64::try_from(millis)
        .map_err(|e| NodegetError::Other(format!("Timestamp conversion error: {e}")).into())
}

/// 获取经 NTP 校正后的本地时间戳（i64 毫秒）。
pub fn get_local_timestamp_ms_i64() -> Result<i64> {
    get_local_timestamp_ms().and_then(|ts| {
        i64::try_from(ts)
            .map_err(|e| NodegetError::Other(format!("Timestamp conversion error: {e}")).into())
    })
}

/// 生成指定长度的随机字母数字字符串。
///
/// - `len`：目标字符串长度
/// - 返回由 [A-Za-z0-9] 组成的随机字符串
#[must_use]
pub fn generate_random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
