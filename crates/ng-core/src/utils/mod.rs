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

#[cfg(test)]
mod tests {
    use super::{generate_random_string, get_local_timestamp_ms, get_local_timestamp_ms_i64, set_ntp_offset_ms, JsonError};

    // ── JsonError ───────────────────────────────────────────────────

    #[test]
    fn json_error_serialization() {
        let je = JsonError {
            error_id: 102,
            error_message: "Permission denied: test".into(),
        };
        let json = serde_json::to_string(&je).unwrap();
        assert!(json.contains("\"error_id\""));
        assert!(json.contains("\"error_message\""));
        assert!(json.contains("102"));
    }

    #[test]
    fn json_error_deserialization() {
        let json = r#"{"error_id":999,"error_message":"Other error: x"}"#;
        let je: JsonError = serde_json::from_str(json).unwrap();
        assert_eq!(je.error_id, 999);
        assert_eq!(je.error_message, "Other error: x");
    }

    // ── generate_random_string ──────────────────────────────────────

    #[test]
    fn random_string_length() {
        for len in [0, 1, 8, 32, 128] {
            let s = generate_random_string(len);
            assert_eq!(s.len(), len, "expected length {len}");
        }
    }

    #[test]
    fn random_string_alphanumeric_only() {
        let s = generate_random_string(256);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()),
            "all chars must be alphanumeric: {s}");
    }

    #[test]
    fn random_string_different_on_consecutive_calls() {
        let a = generate_random_string(64);
        let b = generate_random_string(64);
        // Extremely unlikely to match
        assert_ne!(a, b, "two consecutive random strings should differ");
    }

    // ── Timestamp functions ─────────────────────────────────────────

    #[test]
    fn get_local_timestamp_ms_returns_positive() {
        let ts = get_local_timestamp_ms().unwrap();
        assert!(ts > 0, "timestamp should be positive");
    }

    #[test]
    fn get_local_timestamp_ms_i64_returns_positive() {
        let ts = get_local_timestamp_ms_i64().unwrap();
        assert!(ts > 0, "timestamp should be positive");
    }

    #[test]
    fn ntp_offset_applied() {
        // Save original offset
        let before = get_local_timestamp_ms_i64().unwrap();
        // Set a large positive offset
        set_ntp_offset_ms(1_000_000); // +1000 seconds
        let after = get_local_timestamp_ms_i64().unwrap();
        // Should be at least 999_000 ms larger (allowing some clock drift)
        assert!(after > before + 999_000, "NTP offset should add to timestamp: before={before}, after={after}");
        // Reset to zero so other tests are not affected
        set_ntp_offset_ms(0);
    }

    #[test]
    fn ntp_offset_zero_no_change() {
        set_ntp_offset_ms(0);
        let ts = get_local_timestamp_ms().unwrap();
        // Just verify it doesn't error
        assert!(ts > 0);
    }
}
