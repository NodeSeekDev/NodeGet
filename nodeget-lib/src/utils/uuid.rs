use crate::error::Result;
use uuid::Uuid;

/// 生成随机 UUIDv4
///
/// # Errors
///
/// 永远不会返回错误（保留签名兼容性）
pub fn generate_random_uuid() -> Result<Uuid> {
    Ok(Uuid::new_v4())
}
