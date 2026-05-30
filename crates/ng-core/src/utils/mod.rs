use crate::error::{NodegetError, Result};
use rand::distr::Alphanumeric;
use rand::{Rng, rng};
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};

#[cfg(feature = "for-server")]
pub mod error_message;

pub mod version;

pub mod uuid;

#[cfg(feature = "for-server")]
pub mod server_json;

#[derive(Serialize, Deserialize)]
pub struct JsonError {
    pub error_id: i128,
    pub error_message: String,
}

static NTP_OFFSET_MS: AtomicI64 = AtomicI64::new(0);

pub fn set_ntp_offset_ms(offset_ms: i64) {
    NTP_OFFSET_MS.store(offset_ms, Ordering::Relaxed);
}

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

pub fn get_local_timestamp_ms_i64() -> Result<i64> {
    get_local_timestamp_ms().and_then(|ts| {
        i64::try_from(ts)
            .map_err(|e| NodegetError::Other(format!("Timestamp conversion error: {e}")).into())
    })
}

#[must_use]
pub fn generate_random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
