use serde::{Deserialize, Serialize};

// 统一认证枚举，支持令牌和用户名密码两种认证方式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenOrAuth {
    // 令牌认证方式，包含令牌密钥和密钥
    Token(String, String),
    // 用户名密码认证方式，包含用户名和密码
    Auth(String, String),
}

impl TokenOrAuth {
    // 从完整令牌字符串解析出认证信息
    //
    // # 参数
    // * `full_token` - 完整令牌字符串，格式为 'key:secret' 或 'username|password'

    pub fn from_full_token(full_token: &str) -> Result<Self, String> {
        if let Some((key, secret)) = full_token.split_once(':') {
            Ok(Self::Token(key.to_string(), secret.to_string()))
        } else if let Some((username, password)) = full_token.split_once('|') {
            Ok(Self::Auth(username.to_string(), password.to_string()))
        } else {
            Err("Invalid token format: must be 'key:secret' or 'username|password'".to_string())
        }
    }

    // 获取令牌密钥，如果当前是令牌认证方式则返回 Some，否则返回 None
    #[must_use]
    pub fn token_key(&self) -> Option<&str> {
        match self {
            Self::Token(key, _) => Some(key),
            Self::Auth(_, _) => None,
        }
    }

    // 获取令牌密钥，如果当前是令牌认证方式则返回 Some，否则返回 None
    #[must_use]
    pub fn token_secret(&self) -> Option<&str> {
        match self {
            Self::Token(_, secret) => Some(secret),
            Self::Auth(_, _) => None,
        }
    }

    // 获取用户名，如果当前是用户名密码认证方式则返回 Some，否则返回 None
    #[must_use]
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(username, _) => Some(username),
        }
    }

    // 获取密码，如果当前是用户名密码认证方式则返回 Some，否则返回 None
    #[must_use]
    pub fn password(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(_, password) => Some(password),
        }
    }

    // 检查当前是否为令牌认证方式
    #[must_use]
    pub const fn is_token(&self) -> bool {
        matches!(self, Self::Token(_, _))
    }

    // 检查当前是否为用户名密码认证方式
    #[must_use]
    pub const fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_, _))
    }
}
