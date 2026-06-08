//! 统一错误类型定义
//!
//! 定义 `NodegetError` 枚举，覆盖解析、权限、数据库、Agent 连接、
//! 序列化、IO 等场景。所有 crate 通过此类型或 `anyhow::Result` 报告错误。
//! 同时提供错误码映射与 JSON 序列化能力，供 RPC 层统一返回格式。

use thiserror::Error;

/// NodeGet 全局错误枚举，各变体携带描述字符串。
#[derive(Error, Debug, Clone)]
pub enum NodegetError {
    /// 解析失败（格式、语法等）
    #[error("Parse error: {0}")]
    ParseError(String),

    /// 输入校验不通过
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// 权限不足或 Token 校验失败
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// 数据库操作出错
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// 无法连接 Agent
    #[error("Unable to connect agent: {0}")]
    AgentConnectionError(String),

    /// 数据库中未找到指定记录
    #[error("Not found in database: {0}")]
    NotFound(String),

    /// 监控 UUID 不存在
    #[error("UUID not found: {0}")]
    UuidNotFound(String),

    /// 配置项不存在
    #[error("Config not found: {0}")]
    ConfigNotFound(String),

    /// 序列化/反序列化失败
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// IO 操作失败
    #[error("IO error: {0}")]
    IoError(String),

    /// 未归类的其他错误
    #[error("Other error: {0}")]
    Other(String),
}

impl NodegetError {
    /// 返回该错误对应的数值错误码，用于 RPC 响应中的 `error_id` 字段。
    ///
    /// - 解析/序列化/IO 类共享 101
    /// - 权限拒绝 102
    /// - 数据库 103、Agent 连接 104、未找到 105、UUID 未找到 106、配置未找到 107
    /// - 输入校验 108
    /// - 其他 999
    #[must_use]
    pub const fn error_code(&self) -> i128 {
        match self {
            Self::InvalidInput(_) => 108,
            Self::PermissionDenied(_) => 102,
            Self::DatabaseError(_) => 103,
            Self::AgentConnectionError(_) => 104,
            Self::NotFound(_) => 105,
            Self::UuidNotFound(_) => 106,
            Self::ConfigNotFound(_) => 107,
            Self::Other(_) => 999,
            Self::ParseError(_) | Self::SerializationError(_) | Self::IoError(_) => 101,
        }
    }

    /// 将错误转换为 RPC 层使用的 `JsonError` 结构体。
    ///
    /// - 返回包含 `error_id` 与 `error_message` 的 JSON 可序列化对象
    #[must_use]
    pub fn to_json_error(&self) -> crate::utils::JsonError {
        crate::utils::JsonError {
            error_id: self.error_code(),
            error_message: self.to_string(),
        }
    }
}

/// 从 `serde_json::Error` 转换，统一归入序列化错误。
impl From<serde_json::Error> for NodegetError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err.to_string())
    }
}

/// 从 `std::io::Error` 转换，统一归入 IO 错误。
impl From<std::io::Error> for NodegetError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

/// 通用 Result 类型别名，简化函数签名。
pub type Result<T> = anyhow::Result<T>;

/// 尝试将 `anyhow::Error` 还原为 `NodegetError`。
///
/// - 若底层已是 `NodegetError`，则 clone 返回
/// - 否则包装为 `NodegetError::Other`
#[must_use]
pub fn anyhow_to_nodeget_error(err: &anyhow::Error) -> NodegetError {
    if let Some(e) = err.downcast_ref::<NodegetError>() {
        return e.clone();
    }
    NodegetError::Other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{anyhow_to_nodeget_error, NodegetError};

    // ── Variant constructions + Display ─────────────────────────────

    #[test]
    fn parse_error_display() {
        let e = NodegetError::ParseError("bad format".into());
        assert_eq!(e.to_string(), "Parse error: bad format");
    }

    #[test]
    fn invalid_input_display() {
        let e = NodegetError::InvalidInput("empty name".into());
        assert_eq!(e.to_string(), "Invalid input: empty name");
    }

    #[test]
    fn permission_denied_display() {
        let e = NodegetError::PermissionDenied("no access".into());
        assert_eq!(e.to_string(), "Permission denied: no access");
    }

    #[test]
    fn database_error_display() {
        let e = NodegetError::DatabaseError("conn refused".into());
        assert_eq!(e.to_string(), "Database error: conn refused");
    }

    #[test]
    fn agent_connection_error_display() {
        let e = NodegetError::AgentConnectionError("timeout".into());
        assert_eq!(e.to_string(), "Unable to connect agent: timeout");
    }

    #[test]
    fn not_found_display() {
        let e = NodegetError::NotFound("id=42".into());
        assert_eq!(e.to_string(), "Not found in database: id=42");
    }

    #[test]
    fn uuid_not_found_display() {
        let e = NodegetError::UuidNotFound("abc".into());
        assert_eq!(e.to_string(), "UUID not found: abc");
    }

    #[test]
    fn config_not_found_display() {
        let e = NodegetError::ConfigNotFound("db".into());
        assert_eq!(e.to_string(), "Config not found: db");
    }

    #[test]
    fn serialization_error_display() {
        let e = NodegetError::SerializationError("invalid json".into());
        assert_eq!(e.to_string(), "Serialization error: invalid json");
    }

    #[test]
    fn io_error_display() {
        let e = NodegetError::IoError("file not found".into());
        assert_eq!(e.to_string(), "IO error: file not found");
    }

    #[test]
    fn other_display() {
        let e = NodegetError::Other("misc".into());
        assert_eq!(e.to_string(), "Other error: misc");
    }

    // ── error_code ──────────────────────────────────────────────────

    #[test]
    fn error_code_parse_serialization_io_share_101() {
        assert_eq!(NodegetError::ParseError("".into()).error_code(), 101);
        assert_eq!(NodegetError::SerializationError("".into()).error_code(), 101);
        assert_eq!(NodegetError::IoError("".into()).error_code(), 101);
    }

    #[test]
    fn error_code_permission_denied_102() {
        assert_eq!(NodegetError::PermissionDenied("".into()).error_code(), 102);
    }

    #[test]
    fn error_code_database_103() {
        assert_eq!(NodegetError::DatabaseError("".into()).error_code(), 103);
    }

    #[test]
    fn error_code_agent_connection_104() {
        assert_eq!(NodegetError::AgentConnectionError("".into()).error_code(), 104);
    }

    #[test]
    fn error_code_not_found_105() {
        assert_eq!(NodegetError::NotFound("".into()).error_code(), 105);
    }

    #[test]
    fn error_code_uuid_not_found_106() {
        assert_eq!(NodegetError::UuidNotFound("".into()).error_code(), 106);
    }

    #[test]
    fn error_code_config_not_found_107() {
        assert_eq!(NodegetError::ConfigNotFound("".into()).error_code(), 107);
    }

    #[test]
    fn error_code_invalid_input_108() {
        assert_eq!(NodegetError::InvalidInput("".into()).error_code(), 108);
    }

    #[test]
    fn error_code_other_999() {
        assert_eq!(NodegetError::Other("".into()).error_code(), 999);
    }

    // ── to_json_error ───────────────────────────────────────────────

    #[test]
    fn to_json_error_fields() {
        let e = NodegetError::PermissionDenied("denied".into());
        let je = e.to_json_error();
        assert_eq!(je.error_id, 102);
        assert_eq!(je.error_message, "Permission denied: denied");
    }

    // ── From impls ──────────────────────────────────────────────────

    #[test]
    fn from_serde_json_error() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("not a number").unwrap_err();
        let e: NodegetError = json_err.into();
        assert!(matches!(e, NodegetError::SerializationError(_)));
        assert!(e.to_string().contains("Serialization error:"));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e: NodegetError = io_err.into();
        assert!(matches!(e, NodegetError::IoError(_)));
        assert!(e.to_string().contains("IO error:"));
    }

    // ── anyhow_to_nodeget_error ─────────────────────────────────────

    #[test]
    fn anyhow_to_nodeget_error_preserves_nodeget_error() {
        let original = NodegetError::NotFound("row".into());
        let anyhow_err: anyhow::Error = original.clone().into();
        let recovered = anyhow_to_nodeget_error(&anyhow_err);
        assert!(matches!(recovered, NodegetError::NotFound(s) if s == "row"));
    }

    #[test]
    fn anyhow_to_nodeget_error_wraps_other() {
        let anyhow_err = anyhow::anyhow!("some generic error");
        let recovered = anyhow_to_nodeget_error(&anyhow_err);
        assert!(matches!(recovered, NodegetError::Other(_)));
    }

    // ── Debug + Clone ───────────────────────────────────────────────

    #[test]
    fn debug_clone() {
        let e = NodegetError::InvalidInput("x".into());
        let cloned = e.clone();
        // Verify clone produces same Display output (NodegetError lacks PartialEq)
        assert_eq!(cloned.to_string(), e.to_string());
        let debug = format!("{e:?}");
        assert!(debug.contains("InvalidInput"));
    }

    #[test]
    fn all_variants_distinct_error_codes() {
        let codes = [
            NodegetError::ParseError("".into()).error_code(),
            NodegetError::InvalidInput("".into()).error_code(),
            NodegetError::PermissionDenied("".into()).error_code(),
            NodegetError::DatabaseError("".into()).error_code(),
            NodegetError::AgentConnectionError("".into()).error_code(),
            NodegetError::NotFound("".into()).error_code(),
            NodegetError::UuidNotFound("".into()).error_code(),
            NodegetError::ConfigNotFound("".into()).error_code(),
            NodegetError::Other("".into()).error_code(),
        ];
        // ParseError/SerializationError/IoError share 101, so exclude those
        let unique: std::collections::HashSet<i128> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len(), "all non-101-group error codes should be distinct");
    }
}
