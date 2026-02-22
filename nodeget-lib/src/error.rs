use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum NodegetError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Unable to connect agent: {0}")]
    AgentConnectionError(String),

    #[error("Not found in database: {0}")]
    NotFound(String),

    #[error("UUID not found: {0}")]
    UuidNotFound(String),

    #[error("Config not found: {0}")]
    ConfigNotFound(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Other error: {0}")]
    Other(String),
}

impl NodegetError {
    /// 获取错误对应的错误代码
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

    /// 转换为 JSON 错误格式
    #[must_use]
    pub fn to_json_error(&self) -> crate::utils::JsonError {
        crate::utils::JsonError {
            error_id: self.error_code(),
            error_message: self.to_string(),
        }
    }
}

impl From<serde_json::Error> for NodegetError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err.to_string())
    }
}

impl From<std::io::Error> for NodegetError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

/// 统一结果类型
pub type Result<T> = anyhow::Result<T>;

/// 从 `anyhow::Error` 转换为 `NodegetError`
#[must_use]
pub fn anyhow_to_nodeget_error(err: &anyhow::Error) -> NodegetError {
    if let Some(e) = err.downcast_ref::<NodegetError>() {
        return e.clone();
    }
    NodegetError::Other(err.to_string())
}
