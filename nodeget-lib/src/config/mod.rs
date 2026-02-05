use crate::utils::uuid::get_stable_device_uuid;
use serde::{Deserialize, Deserializer};
use uuid::Uuid;

// 服务器配置模块
#[cfg(feature = "for-server")]
pub mod server;

// Agent 配置模块
#[cfg(feature = "for-agent")]
pub mod agent;

// 自定义 UUID 反序列化函数，支持 "auto_gen" 关键字自动生成设备 UUID
//
// 当输入为 "auto_gen" 时，使用设备稳定 UUID 生成器生成 UUID；
// 否则尝试解析输入字符串为标准 UUID 格式
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
