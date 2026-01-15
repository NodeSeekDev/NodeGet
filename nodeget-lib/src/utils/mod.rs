pub mod error_message;
pub mod version;

// 毫秒时间戳，超过 u64 范围时返回 0
#[must_use] 
pub fn get_local_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO);
    let millis = duration.as_millis();
    u64::try_from(millis).unwrap_or(0)
}

// Windows / MacOS / Linux 下的唯一 UUID 生成器，在同一系统下不变
#[must_use] 
pub fn get_stable_device_uuid() -> String {
    use uuid::Uuid;
    let machine_id = machine_uid::get().unwrap_or_else(|e| {
        eprintln!("无法获取系统 ID: {e}, 使用 fallback");
        "fallback-device-id".to_string()
    });

    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, machine_id.as_bytes());

    uuid.to_string()
}
