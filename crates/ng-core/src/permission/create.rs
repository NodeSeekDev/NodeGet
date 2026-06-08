//! Token 创建请求体定义

use crate::permission::data_structure::Limit;
use serde::{Deserialize, Serialize};

/// Token 创建请求，携带可选凭证与权限限制。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenCreationRequest {
    /// 关联的用户名（可选）
    pub username: Option<String>,
    /// 关联的密码（可选）
    pub password: Option<String>,
    /// Token 有效期起始时间（Unix 毫秒，可选）
    pub timestamp_from: Option<i64>,
    /// Token 有效期截止时间（Unix 毫秒，可选）
    pub timestamp_to: Option<i64>,
    /// Token 版本号，用于区分不同颁发策略
    pub version: Option<i32>,
    /// Token 的权限限制列表
    pub token_limit: Vec<Limit>,
}

#[cfg(test)]
mod tests {
    use super::{Limit, TokenCreationRequest};
    use crate::permission::data_structure::{Permission, Scope};

    #[test]
    fn creation_request_minimal() {
        let req = TokenCreationRequest {
            username: None,
            password: None,
            timestamp_from: None,
            timestamp_to: None,
            version: None,
            token_limit: vec![],
        };
        assert!(req.username.is_none());
        assert!(req.token_limit.is_empty());
    }

    #[test]
    fn creation_request_full() {
        let req = TokenCreationRequest {
            username: Some("admin".into()),
            password: Some("secret".into()),
            timestamp_from: Some(1000),
            timestamp_to: Some(2000),
            version: Some(1),
            token_limit: vec![Limit {
                scopes: vec![Scope::Global],
                permissions: vec![Permission::Terminal(
                    crate::permission::data_structure::Terminal::Connect,
                )],
            }],
        };
        assert_eq!(req.username.as_deref(), Some("admin"));
        assert_eq!(req.password.as_deref(), Some("secret"));
        assert_eq!(req.timestamp_from, Some(1000));
        assert_eq!(req.timestamp_to, Some(2000));
        assert_eq!(req.version, Some(1));
        assert_eq!(req.token_limit.len(), 1);
    }

    #[test]
    fn creation_request_eq() {
        let req1 = TokenCreationRequest {
            username: Some("u".into()),
            password: None,
            timestamp_from: None,
            timestamp_to: None,
            version: Some(2),
            token_limit: vec![],
        };
        let req2 = TokenCreationRequest {
            username: Some("u".into()),
            password: None,
            timestamp_from: None,
            timestamp_to: None,
            version: Some(2),
            token_limit: vec![],
        };
        assert_eq!(req1, req2);
    }

    #[test]
    fn creation_request_serde_round_trip() {
        let req = TokenCreationRequest {
            username: Some("user".into()),
            password: Some("pass".into()),
            timestamp_from: Some(100),
            timestamp_to: Some(200),
            version: Some(3),
            token_limit: vec![Limit {
                scopes: vec![Scope::KvNamespace("ns".into())],
                permissions: vec![Permission::Kv(
                    crate::permission::data_structure::Kv::Read("ns".into()),
                )],
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        let de: TokenCreationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, de);
    }

    #[test]
    fn creation_request_debug() {
        let req = TokenCreationRequest {
            username: None,
            password: None,
            timestamp_from: None,
            timestamp_to: None,
            version: None,
            token_limit: vec![],
        };
        let d = format!("{req:?}");
        assert!(d.contains("TokenCreationRequest"));
    }
}
