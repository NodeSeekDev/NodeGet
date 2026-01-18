use serde::{Deserialize, Deserializer};
use uuid::Uuid;
use crate::utils::get_stable_device_uuid;

#[cfg(feature = "for-server")]
pub mod server;

#[cfg(feature = "for-agent")]
pub mod agent;

fn deserialize_uuid_or_auto<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;

    // 2. 判断逻辑
    if s.eq_ignore_ascii_case("auto_gen") {
        Ok(get_stable_device_uuid())
    } else {
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}