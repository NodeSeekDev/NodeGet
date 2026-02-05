use crate::permission::data_structure::Limit;
use serde::{Deserialize, Serialize};

// 令牌创建请求结构体，定义创建新令牌时所需的参数
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenCreationRequest {
    // 用户名，可选参数
    pub username: Option<String>,
    // 密码，可选参数
    pub password: Option<String>,

    // 令牌生效时间戳（毫秒），可选参数
    pub timestamp_from: Option<i64>,
    // 令牌过期时间戳（毫秒），可选参数
    pub timestamp_to: Option<i64>,

    // 令牌版本号，可选参数
    pub version: Option<i32>,

    // 令牌权限限制列表
    pub token_limit: Vec<Limit>,
}
