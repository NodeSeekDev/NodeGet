//! Token / Auth 双模式认证类型
//!
//! RPC 调用支持两种认证方式：key:secret Token 或 username|password 账号认证。
//! `TokenOrAuth` 将两者统一抽象，供鉴权层无差别处理。

use serde::{Deserialize, Serialize};

/// 认证凭证：Token 模式或账号密码模式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenOrAuth {
    /// key:secret 格式的 Token 认证
    Token(String, String),
    /// username|password 格式的账号认证
    Auth(String, String),
}

impl TokenOrAuth {
    /// 从完整凭证字符串解析认证模式。
    ///
    /// - `full_token`：`key:secret` 或 `username|password` 格式的字符串
    /// - 返回解析后的 `TokenOrAuth`，格式非法时返回 Err
    pub fn from_full_token(full_token: &str) -> Result<Self, String> {
        if let Some((key, secret)) = full_token.split_once(':') {
            Ok(Self::Token(key.to_string(), secret.to_string()))
        } else if let Some((username, password)) = full_token.split_once('|') {
            Ok(Self::Auth(username.to_string(), password.to_string()))
        } else {
            Err("Invalid token format: must be 'key:secret' or 'username|password'".to_string())
        }
    }

    /// 返回 Token 的 key 部分，账号模式返回 None。
    #[must_use]
    pub fn token_key(&self) -> Option<&str> {
        match self {
            Self::Token(key, _) => Some(key),
            Self::Auth(_, _) => None,
        }
    }

    /// 返回 Token 的 secret 部分，账号模式返回 None。
    #[must_use]
    pub fn token_secret(&self) -> Option<&str> {
        match self {
            Self::Token(_, secret) => Some(secret),
            Self::Auth(_, _) => None,
        }
    }

    /// 返回账号模式的用户名，Token 模式返回 None。
    #[must_use]
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(username, _) => Some(username),
        }
    }

    /// 返回账号模式的密码，Token 模式返回 None。
    #[must_use]
    pub fn password(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(_, password) => Some(password),
        }
    }

    /// 判断是否为 Token 模式。
    #[must_use]
    pub const fn is_token(&self) -> bool {
        matches!(self, Self::Token(_, _))
    }

    /// 判断是否为账号密码模式。
    #[must_use]
    pub const fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_, _))
    }
}

#[cfg(test)]
mod tests {
    use super::TokenOrAuth;

    // ── from_full_token ──────────────────────────────────────────────

    #[test]
    fn from_full_token_key_secret() {
        let result = TokenOrAuth::from_full_token("key:secret");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Token("key".into(), "secret".into()));
    }

    #[test]
    fn from_full_token_user_password() {
        let result = TokenOrAuth::from_full_token("user|password");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Auth("user".into(), "password".into()));
    }

    #[test]
    fn from_full_token_colons_in_secret() {
        let result = TokenOrAuth::from_full_token("key:secret:with:colons");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            TokenOrAuth::Token("key".into(), "secret:with:colons".into())
        );
    }

    #[test]
    fn from_full_token_pipes_in_password() {
        let result = TokenOrAuth::from_full_token("user|pass|with|pipes");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            TokenOrAuth::Auth("user".into(), "pass|with|pipes".into())
        );
    }

    #[test]
    fn from_full_token_no_separator_is_error() {
        let result = TokenOrAuth::from_full_token("no_separator");
        assert!(result.is_err());
    }

    #[test]
    fn from_full_token_empty_string_is_error() {
        let result = TokenOrAuth::from_full_token("");
        assert!(result.is_err());
    }

    #[test]
    fn from_full_token_empty_key() {
        let result = TokenOrAuth::from_full_token(":secret");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Token("".into(), "secret".into()));
    }

    #[test]
    fn from_full_token_empty_secret() {
        let result = TokenOrAuth::from_full_token("key:");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Token("key".into(), "".into()));
    }

    #[test]
    fn from_full_token_empty_username() {
        let result = TokenOrAuth::from_full_token("|password");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Auth("".into(), "password".into()));
    }

    #[test]
    fn from_full_token_empty_password() {
        let result = TokenOrAuth::from_full_token("user|");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TokenOrAuth::Auth("user".into(), "".into()));
    }

    #[test]
    fn from_full_token_colon_takes_precedence_over_pipe() {
        // When both separators exist, colon (Token) takes priority
        let result = TokenOrAuth::from_full_token("key:secret|extra");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            TokenOrAuth::Token("key".into(), "secret|extra".into())
        );
    }

    // ── Accessors ────────────────────────────────────────────────────

    #[test]
    fn token_accessors() {
        let t = TokenOrAuth::Token("mykey".into(), "mysecret".into());
        assert_eq!(t.token_key(), Some("mykey"));
        assert_eq!(t.token_secret(), Some("mysecret"));
        assert_eq!(t.username(), None);
        assert_eq!(t.password(), None);
        assert!(t.is_token());
        assert!(!t.is_auth());
    }

    #[test]
    fn auth_accessors() {
        let a = TokenOrAuth::Auth("admin".into(), "pass123".into());
        assert_eq!(a.token_key(), None);
        assert_eq!(a.token_secret(), None);
        assert_eq!(a.username(), Some("admin"));
        assert_eq!(a.password(), Some("pass123"));
        assert!(!a.is_token());
        assert!(a.is_auth());
    }

    // ── Derives ─────────────────────────────────────────────────────

    #[test]
    fn clone_eq() {
        let t = TokenOrAuth::Token("k".into(), "s".into());
        assert_eq!(t.clone(), t);
        let a = TokenOrAuth::Auth("u".into(), "p".into());
        assert_eq!(a.clone(), a);
        assert_ne!(t, a);
    }

    #[test]
    fn serde_round_trip_token() {
        let t = TokenOrAuth::Token("k".into(), "s".into());
        let json = serde_json::to_string(&t).unwrap();
        let de: TokenOrAuth = serde_json::from_str(&json).unwrap();
        assert_eq!(t, de);
    }

    #[test]
    fn serde_round_trip_auth() {
        let a = TokenOrAuth::Auth("u".into(), "p".into());
        let json = serde_json::to_string(&a).unwrap();
        let de: TokenOrAuth = serde_json::from_str(&json).unwrap();
        assert_eq!(a, de);
    }

    #[test]
    fn serde_renames_to_snake_case() {
        let t = TokenOrAuth::Token("k".into(), "s".into());
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"token\""), "expected snake_case variant: {json}");
        let a = TokenOrAuth::Auth("u".into(), "p".into());
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"auth\""), "expected snake_case variant: {json}");
    }
}
