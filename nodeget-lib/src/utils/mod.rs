use rand::distr::Alphanumeric;
use rand::{Rng, rng};
use serde::Deserialize;
use serde::Serialize;

// 服务器错误消息处理模块
#[cfg(feature = "for-server")]
pub mod error_message;

// 版本信息模块
pub mod version;

// Uuid 相关
pub mod uuid;

// 服务器 Json Parser
#[cfg(feature = "for-server")]
pub mod server_json;

// JSON-RPC 公共错误结构体
//
// 错误代码说明：
// 101: Parse Error
// 102: Permission Denied
// 103: Database Error
// 104: Unable to connect agent
// 105: Not Found in Database
// 106: Uuid Not Found
// 107: Config Not Found
//
// 999: 详情请看 error_message
#[derive(Serialize, Deserialize)]
pub struct JsonError {
    // 错误 ID
    pub error_id: i128,
    // 错误消息
    pub error_message: String,
}

// 获取本地毫秒级时间戳
//
// # 返回值
// 返回当前时间的毫秒级时间戳，如果超过 u64 范围则返回 0
#[must_use]
pub fn get_local_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO);
    let millis = duration.as_millis();
    u64::try_from(millis).unwrap_or(0)
}

// 生成指定长度的随机字符串
//
// # 参数
// * `len` - 需要生成的随机字符串长度
//
// # 返回值
// 返回生成的随机字符串
pub fn generate_random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
