use serde::{Deserialize, Deserializer};
use uuid::Uuid;

// 服务器配置模块
#[cfg(feature = "for-server")]
pub mod server;

// Agent 配置模块
#[cfg(feature = "for-agent")]
pub mod agent;

// 自定义 UUID 反序列化函数
//
// auto_gen 被禁止直接反序列化；持久化替换由 get_and_parse_config 完成。
// 否则尝试解析输入字符串为标准 UUID 格式
fn deserialize_uuid_or_auto<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;

    if s.eq_ignore_ascii_case("auto_gen") {
        return Err(serde::de::Error::custom(
            "auto_gen is not supported here; use get_and_parse_config for auto-generation",
        ));
    } else {
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}
